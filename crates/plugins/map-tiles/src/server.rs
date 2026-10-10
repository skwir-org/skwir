//! A tile server as the user described it (`map-tiles.servers`), or the
//! built-in OpenStreetMap one: how its addresses are written, what it is
//! sent, and whose tiles they are.
//!
//! Addresses take the forms tile servers publish, as Leaflet and QGIS
//! write them: `{z}`, `{x}`, `{y}`; `{-y}` for a server that counts rows
//! from the bottom (TMS); `{q}` for a quadkey (Bing); `{s}` for one of the
//! server's subdomains, spread over the tiles; `{r}`, `@2x` on a dense
//! screen for a server that has tiles twice as sharp; and `{key}`, the API
//! key, kept in the keyring and put in the address or a header only when a
//! request is made.
use std::hash::{Hash, Hasher};

use skwir_core::settings::SECRET_STORED;
use skwir_core::tiles::TileRequest;

pub const OSM_ID: &str = "openstreetmap";
const OSM_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// The deepest zoom a server is asked for unless its record says.
pub const MAX_ZOOM: u8 = 19;

/// Every part an address may hold.
const PARTS: [&str; 8] = ["{z}", "{x}", "{y}", "{-y}", "{q}", "{s}", "{r}", "{key}"];

/// Headers the HTTP layer writes itself, or that would change what a
/// request is: a record cannot set them.
const RESERVED: [&str; 12] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
    "if-none-match",
    "if-modified-since",
    "proxy-authorization",
    "proxy-connection",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub template: String,
    pub subdomains: Vec<String>,
    pub attribution: String,
    pub attribution_url: Option<String>,
    pub max_zoom: u8,
    pub headers: Vec<(String, String)>,
    /// An API key is kept in the keyring for it.
    pub keyed: bool,
}

/// Subdomains as servers list them: `a b c`, `a,b,c`, or `abc`.
fn subdomains(text: &str) -> Vec<String> {
    let parts: Vec<String> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    match parts.as_slice() {
        [one] if one.chars().count() > 1 && one.chars().all(char::is_alphanumeric) => {
            one.chars().map(String::from).collect()
        }
        _ => parts,
    }
}

/// The quadkey of a tile: its path down the quadtree, one digit a level.
fn quadkey(z: u8, x: u32, y: u32) -> String {
    (1..=z)
        .rev()
        .map(|level| {
            let mask = 1u32 << (level - 1);
            let digit = u8::from(x & mask != 0) + 2 * u8::from(y & mask != 0);
            char::from(b'0' + digit)
        })
        .collect()
}

fn text<'a>(record: &'a toml::Table, key: &str) -> &'a str {
    record.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

impl Server {
    pub fn openstreetmap() -> Server {
        Server {
            id: OSM_ID.into(),
            name: "OpenStreetMap".into(),
            template: OSM_URL.into(),
            subdomains: Vec::new(),
            attribution: "© OpenStreetMap contributors".into(),
            attribution_url: Some("https://www.openstreetmap.org/copyright".into()),
            max_zoom: MAX_ZOOM,
            headers: Vec::new(),
            keyed: false,
        }
    }

    /// A server from its record, if the record is one.
    pub fn from_record(value: &toml::Value) -> Option<Server> {
        let record = value.as_table()?;
        let id = text(record, "id");
        let template = text(record, "url");
        if id.is_empty() || template.is_empty() {
            return None;
        }
        let headers = record
            .get("headers")
            .and_then(toml::Value::as_array)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter_map(|pair| {
                        let pair = pair.as_array()?;
                        Some((
                            pair.first()?.as_str()?.to_owned(),
                            pair.get(1)?.as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Server {
            id: id.into(),
            name: Some(text(record, "name"))
                .filter(|n| !n.is_empty())
                .unwrap_or(id)
                .into(),
            template: template.into(),
            subdomains: subdomains(text(record, "subdomains")),
            attribution: text(record, "attribution").into(),
            attribution_url: None,
            max_zoom: record
                .get("max_zoom")
                .and_then(toml::Value::as_integer)
                .and_then(|z| u8::try_from(z).ok())
                .unwrap_or(MAX_ZOOM),
            headers,
            keyed: text(record, "key") == SECRET_STORED,
        })
    }

    /// Whether a request needs the key from the keyring.
    pub fn needs_key(&self) -> bool {
        self.template.contains("{key}") || self.headers.iter().any(|(_, v)| v.contains("{key}"))
    }

    /// The address of `tile`.
    pub fn address(&self, tile: &TileRequest, key: Option<&str>) -> anyhow::Result<String> {
        let n = 1u64 << tile.z;
        let flipped = n.saturating_sub(1).saturating_sub(u64::from(tile.y));
        let mut url = self
            .template
            .replace("{z}", &tile.z.to_string())
            .replace("{x}", &tile.x.to_string())
            .replace("{-y}", &flipped.to_string())
            .replace("{y}", &tile.y.to_string())
            .replace("{q}", &quadkey(tile.z, tile.x, tile.y))
            .replace("{r}", if tile.scale >= 2 { "@2x" } else { "" });
        if url.contains("{s}") {
            anyhow::ensure!(
                !self.subdomains.is_empty(),
                "{} has no subdomains for {{s}}",
                self.name
            );
            // The same tile always from the same subdomain, so caches along
            // the way hold it once.
            let pick = (tile.x as usize + tile.y as usize) % self.subdomains.len();
            url = url.replace("{s}", &self.subdomains[pick]);
        }
        if url.contains("{key}") {
            let key = key.ok_or_else(|| {
                anyhow::anyhow!("The API key of {} is not in the keyring", self.name)
            })?;
            url = url.replace("{key}", key);
        }
        Ok(url)
    }

    /// The headers of a request, the key put where it goes.
    pub fn request_headers(&self, key: Option<&str>) -> anyhow::Result<Vec<(String, String)>> {
        self.headers
            .iter()
            .map(|(name, value)| {
                if value.contains("{key}") {
                    let key = key.ok_or_else(|| {
                        anyhow::anyhow!("The API key of {} is not in the keyring", self.name)
                    })?;
                    Ok((name.clone(), value.replace("{key}", key)))
                } else {
                    Ok((name.clone(), value.clone()))
                }
            })
            .collect()
    }

    /// What a tile is kept under in the cache: the server and everything
    /// that shapes its tiles, but never the key, which only opens the door.
    pub fn cache_key(&self, tile: &TileRequest) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&self.template, &self.subdomains, &self.headers).hash(&mut hasher);
        let sharp = self.template.contains("{r}") && tile.scale >= 2;
        format!(
            "{}-{:016x}/{}/{}/{}{}",
            self.id,
            hasher.finish(),
            tile.z,
            tile.x,
            tile.y,
            if sharp { "@2x" } else { "" }
        )
    }
}

/// Replace every occurrence of `secret` in `text`: an error from a server
/// may quote the address, which may hold the key.
pub fn redact(text: &str, secret: Option<&str>) -> String {
    match secret.filter(|s| s.len() >= 3) {
        Some(secret) => text.replace(secret, "•••"),
        None => text.to_owned(),
    }
}

fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// The owner's check of `map-tiles.servers`, after the fields' own: what
/// makes a record a server skwir can ask.
pub fn check_servers(value: &toml::Value) -> anyhow::Result<()> {
    for record in value.as_array().into_iter().flatten() {
        let Some(table) = record.as_table() else {
            continue;
        };
        let name = Some(text(table, "name"))
            .filter(|n| !n.is_empty())
            .unwrap_or("A tile server");
        let url = text(table, "url");
        anyhow::ensure!(
            url.starts_with("https://") || url.starts_with("http://"),
            "{name}: the address must start with https://"
        );
        let mut rest = url.to_owned();
        for part in PARTS {
            rest = rest.replace(part, "");
        }
        if let Some(start) = rest.find('{') {
            let end = rest[start..]
                .find('}')
                .map_or(rest.len(), |e| start + e + 1);
            anyhow::bail!(
                "{name}: the address has an unknown part {}",
                &rest[start..end]
            );
        }
        let rows = url.contains("{y}") || url.contains("{-y}");
        anyhow::ensure!(
            (url.contains("{z}") && url.contains("{x}") && rows) || url.contains("{q}"),
            "{name}: the address needs {{z}}, {{x}} and {{y}} (or {{-y}}), or {{q}}"
        );
        anyhow::ensure!(
            !url.contains("{s}") || !subdomains(text(table, "subdomains")).is_empty(),
            "{name}: the address uses {{s}}, so the server needs subdomains"
        );
        let mut uses_key = url.contains("{key}");
        for pair in table
            .get("headers")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_array)
        {
            let header = pair.first().and_then(toml::Value::as_str).unwrap_or("");
            let value = pair.get(1).and_then(toml::Value::as_str).unwrap_or("");
            anyhow::ensure!(is_token(header), "{name}: “{header}” is not a header name");
            anyhow::ensure!(
                !RESERVED.contains(&header.to_ascii_lowercase().as_str()),
                "{name}: skwir sets the {header} header itself"
            );
            anyhow::ensure!(
                !value.contains(['\r', '\n']),
                "{name}: a header's value is one line"
            );
            uses_key |= value.contains("{key}");
        }
        anyhow::ensure!(
            !uses_key || text(table, "key") == SECRET_STORED,
            "{name}: {{key}} is used, but no API key is set"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::tiles::Palette;

    fn tile(z: u8, x: u32, y: u32, scale: u8) -> TileRequest {
        TileRequest {
            z,
            x,
            y,
            scale,
            palette: Palette::default(),
        }
    }

    fn record(toml: &str) -> toml::Value {
        toml::from_str::<toml::Table>(toml)
            .map(toml::Value::Table)
            .unwrap()
    }

    #[test]
    fn addresses_take_the_usual_forms() {
        let server = Server {
            template: "https://{s}.tiles.example.org/{z}/{x}/{-y}{r}.png?k={key}".into(),
            subdomains: subdomains("abc"),
            ..Server::openstreetmap()
        };
        assert_eq!(
            server.address(&tile(3, 4, 1, 2), Some("K")).unwrap(),
            "https://c.tiles.example.org/3/4/6@2x.png?k=K"
        );
        assert_eq!(
            server.address(&tile(3, 4, 1, 1), Some("K")).unwrap(),
            "https://c.tiles.example.org/3/4/6.png?k=K"
        );
        assert!(server.address(&tile(3, 4, 1, 1), None).is_err());
        // Bing's quadkeys.
        assert_eq!(quadkey(3, 3, 5), "213");
        assert_eq!(quadkey(0, 0, 0), "");
        assert_eq!(subdomains("a, b c"), ["a", "b", "c"]);
        assert_eq!(subdomains("tile1 tile2"), ["tile1", "tile2"]);
    }

    #[test]
    fn a_record_is_checked_before_it_is_kept() {
        let ok = |toml: &str| check_servers(&toml::Value::Array(vec![record(toml)]));
        assert!(ok("id='a'\nname='T'\nurl='https://x.org/{z}/{x}/{y}.png'").is_ok());
        assert!(ok("id='a'\nurl='https://x.org/{q}.jpeg'").is_ok());
        let error = |toml: &str| ok(toml).unwrap_err().to_string();
        assert!(error("id='a'\nurl='ftp://x.org/{z}/{x}/{y}'").contains("https://"));
        assert!(error("id='a'\nurl='https://x.org/{z}/{x}'").contains("needs"));
        assert!(error("id='a'\nurl='https://x.org/{z}/{x}/{y}/{zoom}'").contains("{zoom}"));
        assert!(error("id='a'\nurl='https://{s}.x.org/{z}/{x}/{y}'").contains("subdomains"));
        assert!(error("id='a'\nurl='https://x.org/{z}/{x}/{y}?k={key}'").contains("no API key"));
        assert!(ok("id='a'\nurl='https://x.org/{z}/{x}/{y}?k={key}'\nkey='keyring'").is_ok());
        assert!(
            error("id='a'\nurl='https://x.org/{z}/{x}/{y}'\nheaders=[['Host','x']]")
                .contains("itself")
        );
        assert!(
            error("id='a'\nurl='https://x.org/{z}/{x}/{y}'\nheaders=[['Bad Name','x']]")
                .contains("not a header name")
        );
        assert!(
            ok("id='a'\nurl='https://x.org/{z}/{x}/{y}'\nheaders=[['Referer','https://example.org/'],['User-Agent','Mine/1']]")
                .is_ok()
        );
    }

    #[test]
    fn a_server_reads_from_its_record() {
        let server = Server::from_record(&record(
            "id='t'\nname='Topo'\nurl='https://x.org/{z}/{x}/{y}'\nmax_zoom=17\nheaders=[['Authorization','Bearer {key}']]\nkey='keyring'",
        ))
        .unwrap();
        assert_eq!(server.max_zoom, 17);
        assert!(server.keyed && server.needs_key());
        assert_eq!(
            server.request_headers(Some("abc")).unwrap(),
            [("Authorization".to_owned(), "Bearer abc".to_owned())]
        );
        // The key never shapes where a tile is kept.
        let one = server.cache_key(&tile(1, 0, 0, 1));
        assert!(!one.contains("abc"));
        assert_ne!(one, server.cache_key(&tile(1, 1, 0, 1)));
        assert_eq!(
            redact("GET https://x.org/?k=s3cret failed", Some("s3cret")),
            "GET https://x.org/?k=••• failed"
        );
    }
}
