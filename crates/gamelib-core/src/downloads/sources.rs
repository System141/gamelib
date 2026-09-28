//! What a store offers to download for a product, and turning a stored file reference into a
//! signed address.
//!
//! - GOG: `api.gog.com/products/{id}?expand=downloads` lists installers per OS and language,
//!   each made of files with a `downlink`. With the user's token, a downlink answers
//!   `{downlink, checksum}`: the CDN address and an XML file with the file's MD5.
//! - itch.io: `/games/{id}/uploads` lists uploads; `/uploads/{id}/download` redirects to the
//!   CDN. Paid games need the user's download key.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, LOCATION};
use serde::Deserialize;

use super::fetch::Resolved;
use crate::http::{self, Counters, Pacer};
use crate::model::{FileOption, Platform, Store};
use crate::secrets::SecretStore;
use crate::stores::{StoreSyncOptions, gog_account, itch, library};
use crate::{Error, Result};

/// A file to download, as stored with the download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A GOG downlink (`https://api.gog.com/products/…/downlink/installer/…`).
    GogDownlink(String),
    /// An itch.io upload.
    ItchUpload(u64),
}

impl Source {
    pub fn to_db(&self) -> String {
        match self {
            Source::GogDownlink(url) => format!("gog:{url}"),
            Source::ItchUpload(id) => format!("itch-upload:{id}"),
        }
    }

    pub fn from_db(s: &str) -> Option<Self> {
        if let Some(url) = s.strip_prefix("gog:") {
            return Some(Source::GogDownlink(url.to_owned()));
        }
        s.strip_prefix("itch-upload:")?
            .parse()
            .ok()
            .map(Source::ItchUpload)
    }
}

/// One downloadable variant (an installer, an upload) and its files.
#[derive(Debug, Clone)]
pub struct Offer {
    pub option: FileOption,
    pub files: Vec<(Source, Option<u64>)>,
}

/// The platform this build runs on.
pub fn this_platform() -> Platform {
    if cfg!(windows) {
        Platform::Win
    } else if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Linux
    }
}

/// What can be downloaded for a product, the recommended variant first.
pub fn offers(
    store: Store,
    product_id: &str,
    owned_key: Option<&str>,
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
) -> Result<Vec<Offer>> {
    let cancel = AtomicBool::new(false);
    let counters = Counters::default();
    let mut offers = match store {
        Store::Gog => {
            let http = http::api_client(Duration::from_secs(30))?;
            let url = format!("{}/products/{product_id}", opts.endpoints.gog_api);
            let body = http::get_text(
                &http,
                &url,
                |r| r.query(&[("expand", "downloads")]),
                &cancel,
                &counters,
            )
            .map_err(not_found_as_invalid)?;
            gog_offers(&body)?
        }
        Store::Itch => {
            let key = secrets
                .load()?
                .itch
                .ok_or(Error::Invalid("itch_signed_out"))?;
            let client = itch::client()?;
            let url = format!("{}/games/{product_id}/uploads", opts.endpoints.itch_api);
            let mut query = Vec::new();
            if let Some(k) = owned_key {
                query.push(("download_key_id", k.to_owned()));
            }
            let body = http::get_text(
                &client,
                &url,
                |r| r.header(AUTHORIZATION, &key.api_key).query(&query),
                &cancel,
                &counters,
            )
            .map_err(|e| match e {
                Error::Http {
                    status: 401 | 403, ..
                } => Error::Invalid("not_owned"),
                other => not_found_as_invalid(other),
            })?;
            itch_offers(&body)?
        }
    };
    recommend(&mut offers, this_platform());
    Ok(offers)
}

fn not_found_as_invalid(e: Error) -> Error {
    match e {
        Error::Http { status: 404, .. } => Error::Invalid("no_files"),
        other => other,
    }
}

/// Marks the best variant for this computer and puts it first: the right platform, Turkish
/// then English installers, then the largest non-demo upload.
fn recommend(offers: &mut [Offer], platform: Platform) {
    let rank = |o: &Offer| {
        let lang = match o.option.language.as_deref() {
            Some("tr") => 0,
            Some("en") | None => 1,
            _ => 2,
        };
        (
            o.option.platform != Some(platform),
            o.option.demo,
            lang,
            std::cmp::Reverse(o.option.size),
        )
    };
    offers.sort_by_key(rank);
    for (i, o) in offers.iter_mut().enumerate() {
        o.option.recommended = i == 0 && o.option.platform == Some(platform);
    }
}

pub fn gog_offers(json: &str) -> Result<Vec<Offer>> {
    #[derive(Deserialize)]
    struct Product {
        downloads: Option<Downloads>,
    }
    #[derive(Deserialize)]
    struct Downloads {
        #[serde(default)]
        installers: Vec<Installer>,
    }
    #[derive(Deserialize)]
    struct Installer {
        id: String,
        os: String,
        language: Option<String>,
        language_full: Option<String>,
        version: Option<String>,
        #[serde(default)]
        total_size: u64,
        #[serde(default)]
        files: Vec<InstallerFile>,
    }
    #[derive(Deserialize)]
    struct InstallerFile {
        id: String,
        #[serde(default)]
        size: u64,
        downlink: String,
    }
    let product: Product = serde_json::from_str(json)?;
    let installers = product.downloads.map(|d| d.installers).unwrap_or_default();
    Ok(installers
        .into_iter()
        .filter_map(|i| {
            let platform = match i.os.as_str() {
                "windows" => Platform::Win,
                "mac" | "osx" => Platform::Mac,
                "linux" => Platform::Linux,
                _ => return None,
            };
            let mut files: Vec<InstallerFile> = i.files;
            // "en1installer10" sorts before "en1installer2" as text.
            files.sort_by_key(|f| trailing_number(&f.id));
            let version = i.version.filter(|v| !v.is_empty());
            let label = [
                Some(platform_name(platform).to_owned()),
                i.language_full.clone(),
                version.clone(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            Some(Offer {
                option: FileOption {
                    id: i.id,
                    label,
                    platform: Some(platform),
                    language: i.language,
                    version,
                    size: if i.total_size > 0 {
                        i.total_size
                    } else {
                        files.iter().map(|f| f.size).sum()
                    },
                    files: files.len() as u32,
                    demo: false,
                    recommended: false,
                },
                files: files
                    .into_iter()
                    .map(|f| {
                        (
                            Source::GogDownlink(f.downlink),
                            (f.size > 0).then_some(f.size),
                        )
                    })
                    .collect(),
            })
        })
        .collect())
}

pub fn itch_offers(json: &str) -> Result<Vec<Offer>> {
    #[derive(Deserialize)]
    struct Uploads {
        #[serde(default)]
        uploads: Vec<Upload>,
    }
    #[derive(Deserialize)]
    struct Upload {
        id: u64,
        filename: Option<String>,
        #[serde(alias = "displayName")]
        display_name: Option<String>,
        size: Option<u64>,
        #[serde(default)]
        traits: Vec<String>,
        p_windows: Option<bool>,
        p_osx: Option<bool>,
        p_linux: Option<bool>,
        demo: Option<bool>,
        #[serde(rename = "type")]
        kind: Option<String>,
    }
    let uploads: Uploads = serde_json::from_str(json)?;
    Ok(uploads
        .uploads
        .into_iter()
        .filter(|u| u.kind.as_deref().is_none_or(|k| k == "default"))
        .map(|u| {
            let has = |t: &str, flag: Option<bool>| {
                flag.unwrap_or(false) || u.traits.iter().any(|x| x == t)
            };
            let platform = if has("p_windows", u.p_windows) {
                Some(Platform::Win)
            } else if has("p_osx", u.p_osx) {
                Some(Platform::Mac)
            } else if has("p_linux", u.p_linux) {
                Some(Platform::Linux)
            } else {
                None
            };
            let demo = has("demo", u.demo);
            Offer {
                option: FileOption {
                    id: u.id.to_string(),
                    label: u
                        .display_name
                        .filter(|d| !d.is_empty())
                        .or(u.filename)
                        .unwrap_or_else(|| format!("#{}", u.id)),
                    platform,
                    language: None,
                    version: None,
                    size: u.size.unwrap_or(0),
                    files: 1,
                    demo,
                    recommended: false,
                },
                files: vec![(Source::ItchUpload(u.id), u.size)],
            }
        })
        .collect())
}

/// Turns a stored file reference into a signed address, now.
pub fn resolve(
    source: &Source,
    owned_key: Option<&str>,
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
    cancel: &AtomicBool,
) -> Result<Resolved> {
    let counters = Counters::default();
    match source {
        Source::GogDownlink(downlink) => {
            let client = gog_account::client()?;
            let tokens = library::gog_tokens(secrets, opts, &client, cancel, &counters)?;
            let bearer = format!("Bearer {}", tokens.access_token);
            let body = http::get_text(
                &client,
                downlink,
                |r| r.header(AUTHORIZATION, &bearer),
                cancel,
                &counters,
            )
            .map_err(|e| match e {
                Error::Http {
                    status: 401 | 403 | 404,
                    ..
                } => Error::Invalid("not_owned"),
                other => other,
            })?;
            #[derive(Deserialize)]
            struct Downlink {
                downlink: String,
                checksum: Option<String>,
            }
            let link: Downlink = serde_json::from_str(&body)?;
            let (md5, name) = match link.checksum.as_deref().filter(|c| !c.is_empty()) {
                Some(url) => {
                    let api = http::api_client(Duration::from_secs(30))?;
                    let pacer = Pacer::new(Duration::ZERO);
                    pacer.wait(cancel)?;
                    // The checksum is a bonus: a failure only skips verification.
                    http::get_text(&api, url, |r| r, cancel, &counters)
                        .map(|xml| parse_checksum(&xml))
                        .unwrap_or((None, None))
                }
                None => (None, None),
            };
            Ok(Resolved {
                url: link.downlink,
                headers: Vec::new(),
                md5,
                name,
            })
        }
        Source::ItchUpload(id) => {
            let key = secrets
                .load()?
                .itch
                .ok_or(Error::Invalid("itch_signed_out"))?;
            let client = reqwest::blocking::Client::builder()
                .user_agent(http::USER_AGENT)
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            let url = format!("{}/uploads/{id}/download", opts.endpoints.itch_api);
            let mut request = client.get(&url).header(AUTHORIZATION, &key.api_key);
            if let Some(k) = owned_key {
                request = request.query(&[("download_key_id", k)]);
            }
            let response = request.send()?;
            let status = response.status();
            if status.is_redirection()
                && let Some(location) = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
            {
                return Ok(Resolved {
                    url: location.to_owned(),
                    ..Resolved::default()
                });
            }
            match status {
                // Served directly: download from the API address with the key.
                s if s.is_success() => Ok(Resolved {
                    url,
                    headers: vec![("Authorization".into(), key.api_key)],
                    ..Resolved::default()
                }),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    Err(Error::Invalid("not_owned"))
                }
                s => Err(Error::Http {
                    status: s.as_u16(),
                    url,
                }),
            }
        }
    }
}

/// `<file name="setup.exe" md5="…" …>` → (md5, name).
pub fn parse_checksum(xml: &str) -> (Option<String>, Option<String>) {
    let Some(start) = xml.find("<file") else {
        return (None, None);
    };
    let tag = &xml[start..xml[start..].find('>').map_or(xml.len(), |e| start + e)];
    let attr = |name: &str| {
        let pattern = format!(" {name}=\"");
        let from = tag.find(&pattern)? + pattern.len();
        let to = tag[from..].find('"')? + from;
        Some(tag[from..to].trim().to_owned()).filter(|v| !v.is_empty())
    };
    let md5 = attr("md5")
        .map(|m| m.to_ascii_lowercase())
        .filter(|m| m.len() == 32 && m.bytes().all(|b| b.is_ascii_hexdigit()));
    (md5, attr("name"))
}

fn trailing_number(id: &str) -> (String, u64) {
    let digits = id.len() - id.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, number) = id.split_at(id.len() - digits);
    (prefix.to_owned(), number.parse().unwrap_or(0))
}

fn platform_name(p: Platform) -> &'static str {
    match p {
        Platform::Win => "Windows",
        Platform::Mac => "macOS",
        Platform::Linux => "Linux",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOG: &str = r#"{"id":1207664643,"title":"The Witcher 3","downloads":{"installers":[
      {"id":"installer_windows_en","name":"The Witcher 3","os":"windows","language":"en","language_full":"English","version":"4.04b","total_size":8589934592,
       "files":[{"id":"en1installer10","size":10,"downlink":"https://api.gog.com/products/1/downlink/installer/en1installer10"},
                {"id":"en1installer2","size":20,"downlink":"https://api.gog.com/products/1/downlink/installer/en1installer2"},
                {"id":"en1installer0","size":1,"downlink":"https://api.gog.com/products/1/downlink/installer/en1installer0"}]},
      {"id":"installer_windows_tr","os":"windows","language":"tr","language_full":"Türkçe","version":"4.04b","total_size":8589934000,
       "files":[{"id":"tr1installer0","size":1,"downlink":"https://api.gog.com/products/1/downlink/installer/tr1installer0"}]},
      {"id":"installer_linux_en","os":"linux","language":"en","language_full":"English","version":null,"total_size":5,
       "files":[{"id":"en3installer0","size":5,"downlink":"https://api.gog.com/products/1/downlink/installer/en3installer0"}]},
      {"id":"installer_ps_en","os":"ps4","language":"en","files":[]}
    ]}}"#;

    #[test]
    fn gog_installers_in_file_order_and_turkish_first() {
        let mut offers = gog_offers(GOG).unwrap();
        assert_eq!(offers.len(), 3);
        let en = offers
            .iter()
            .find(|o| o.option.id == "installer_windows_en")
            .unwrap();
        let ids: Vec<String> = en
            .files
            .iter()
            .map(|(s, _)| s.to_db().rsplit('/').next().unwrap().to_owned())
            .collect();
        assert_eq!(ids, ["en1installer0", "en1installer2", "en1installer10"]);
        assert_eq!(en.option.label, "Windows · English · 4.04b");
        assert_eq!(en.option.files, 3);

        recommend(&mut offers, Platform::Win);
        assert_eq!(offers[0].option.id, "installer_windows_tr");
        assert!(offers[0].option.recommended);
        assert!(!offers[1].option.recommended);
        recommend(&mut offers, Platform::Linux);
        assert_eq!(offers[0].option.id, "installer_linux_en");
        assert_eq!(offers[0].option.label, "Linux · English");
    }

    #[test]
    fn itch_uploads_prefer_this_platform_and_full_games() {
        let json = r#"{"uploads":[
          {"id":1,"filename":"game-demo.zip","display_name":"Demo","size":100,"traits":["p_windows","demo"],"type":"default"},
          {"id":2,"filename":"game-win.zip","size":900,"p_windows":true,"type":"default"},
          {"id":3,"filename":"game-linux.tar.gz","size":950,"traits":["p_linux"],"type":"default"},
          {"id":4,"filename":"soundtrack.zip","size":50,"type":"soundtrack"}
        ]}"#;
        let mut offers = itch_offers(json).unwrap();
        assert_eq!(offers.len(), 3);
        recommend(&mut offers, Platform::Win);
        assert_eq!(offers[0].option.id, "2");
        assert_eq!(offers[0].option.label, "game-win.zip");
        assert!(offers[0].option.recommended);
        assert!(
            offers
                .iter()
                .any(|o| o.option.demo && o.option.label == "Demo")
        );
        assert_eq!(offers[0].files, vec![(Source::ItchUpload(2), Some(900))]);
    }

    #[test]
    fn sources_round_trip() {
        for s in [
            Source::GogDownlink("https://api.gog.com/x/downlink/installer/en1installer0".into()),
            Source::ItchUpload(77),
        ] {
            assert_eq!(Source::from_db(&s.to_db()), Some(s));
        }
        assert_eq!(Source::from_db("other:1"), None);
    }

    #[test]
    fn checksum_xml() {
        let xml = r#"<file name="setup_the_witcher_3_4.04b.exe" available="1" notavailablemsg="" md5="0CC175B9C0F1B6A831C399E269772661" chunks="1" timestamp="2024-01-01 00:00:00" total_size="1"><chunk id="0" from="0" to="0" method="md5">0cc175b9c0f1b6a831c399e269772661</chunk></file>"#;
        assert_eq!(
            parse_checksum(xml),
            (
                Some("0cc175b9c0f1b6a831c399e269772661".into()),
                Some("setup_the_witcher_3_4.04b.exe".into())
            )
        );
        assert_eq!(parse_checksum("<html>error</html>"), (None, None));
    }
}
