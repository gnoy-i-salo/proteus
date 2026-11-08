//! Conservative public-network policy shared by initial requests and redirects.
use std::net::IpAddr;
use url::{Host, Url};
const ADDRESS_ERROR: &str =
    "Разрешены только публичные HTTP(S)-страницы без учётных данных и нестандартных портов.";

pub(super) fn parse(raw: &str) -> Result<Url, String> {
    if raw.is_empty() || raw.len() > 8192 || raw.trim() != raw || raw.chars().any(char::is_control)
    {
        return Err(ADDRESS_ERROR.into());
    }
    let mut url = Url::parse(raw).map_err(|_| ADDRESS_ERROR)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(ADDRESS_ERROR.into());
    }
    match url.host().ok_or(ADDRESS_ERROR)? {
        Host::Ipv4(ip) if !public_ip(ip.into()) => return Err(ADDRESS_ERROR.into()),
        Host::Ipv6(ip) if !public_ip(ip.into()) => return Err(ADDRESS_ERROR.into()),
        Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            if !host.contains('.')
                || ["localhost", "local", "internal", "home", "lan"]
                    .iter()
                    .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
            {
                return Err(ADDRESS_ERROR.into());
            }
        }
        _ => {}
    }
    url.set_fragment(None);
    Ok(url)
}

pub(super) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !matches!(a, 0 | 10 | 127 | 224..=255)
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192
                    && (b == 168 || (b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99)))
                && !(a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            // Only global unicast; conservatively exclude special-protocol,
            // transition and documentation ranges (IANA special registries).
            (a & 0xe000) == 0x2000
                && !(a == 0x2001 && (b < 0x0200 || b == 0x0db8))
                && a != 0x2002
                && !(a == 0x3fff && b < 0x1000)
        }
    }
}

pub(super) fn validate_addresses(addresses: &[IpAddr]) -> Result<(), String> {
    if addresses.is_empty() || addresses.len() > 32 || addresses.iter().any(|ip| !public_ip(*ip)) {
        return Err("Адрес страницы ведёт в закрытую, служебную или неподдерживаемую сеть.".into());
    }
    Ok(())
}
