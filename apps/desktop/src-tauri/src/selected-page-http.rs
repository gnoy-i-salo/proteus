//! Direct, credential-free HTTP with pinned, prevalidated DNS results per hop.
use super::{policy, Response, Transport};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use url::Url;

pub(super) struct PublicHttp;
static RESOLVING: AtomicUsize = AtomicUsize::new(0);
struct DnsSlot;
impl Drop for DnsSlot {
    fn drop(&mut self) {
        RESOLVING.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Transport for PublicHttp {
    fn resolve(&self, host: &str, timeout: Duration) -> Result<Vec<IpAddr>, String> {
        // System DNS cannot be interrupted portably. Bound both caller wait and
        // outstanding workers; a timed-out worker retains its slot until exit.
        RESOLVING
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                (count < 2).then_some(count + 1)
            })
            .map_err(|_| "Предыдущая проверка адреса ещё не завершилась.")?;
        let slot = DnsSlot;
        let host = host.to_owned();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("page-dns".into())
            .spawn(move || {
                let _slot = slot;
                let result = (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|items| items.take(33).map(|item| item.ip()).collect())
                    .map_err(|_| "Не удалось определить публичный адрес страницы.".to_string());
                let _ = sender.send(result);
            })
            .map_err(|_| "Не удалось запустить проверку адреса.")?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| "Время проверки адреса истекло.".to_string())?
    }

    fn get(&self, url: &Url, addresses: &[IpAddr], timeout: Duration) -> Result<Response, String> {
        policy::validate_addresses(addresses)?;
        let port = url
            .port_or_known_default()
            .ok_or("Неверный порт страницы.")?;
        let pinned: Vec<_> = addresses
            .iter()
            .map(|ip| SocketAddr::new(*ip, port))
            .collect();
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .resolve_to_addrs(url.host_str().ok_or("Нет имени сервера.")?, &pinned)
            .build()
            .map_err(|_| "Не удалось подготовить загрузку страницы.")?;
        let response = client
            .get(url.clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html, text/plain, text/markdown",
            )
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .header(reqwest::header::USER_AGENT, "PROTEUS-selected-page/1")
            .send()
            .map_err(|_| "Не удалось загрузить выбранную страницу.")?;
        if !response
            .remote_addr()
            .is_some_and(|peer| pinned.contains(&peer))
        {
            return Err("Сервер страницы не совпадает с проверенным адресом.".into());
        }
        let header = |name| -> Result<Option<String>, String> {
            response
                .headers()
                .get(name)
                .map(|value: &reqwest::header::HeaderValue| {
                    value
                        .to_str()
                        .map(str::to_owned)
                        .map_err(|_| "Некорректный заголовок страницы.".to_string())
                })
                .transpose()
        };
        let content_length = header(reqwest::header::CONTENT_LENGTH)?
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| "Неверный размер страницы.".to_string())
            })
            .transpose()?;
        Ok(Response {
            status: response.status().as_u16(),
            location: header(reqwest::header::LOCATION)?,
            content_type: header(reqwest::header::CONTENT_TYPE)?,
            content_encoding: header(reqwest::header::CONTENT_ENCODING)?,
            content_length,
            body: Box::new(response),
        })
    }
}
