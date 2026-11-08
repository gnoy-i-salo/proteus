use super::*;
use std::{cell::RefCell, collections::VecDeque, io::Cursor};

struct Fake {
    addresses: Vec<IpAddr>,
    responses: RefCell<VecDeque<Response>>,
    calls: RefCell<Vec<String>>,
}
impl Fake {
    fn new(responses: Vec<Response>) -> Self {
        Self {
            addresses: vec!["93.184.215.14".parse().unwrap()],
            responses: RefCell::new(responses.into()),
            calls: RefCell::new(vec![]),
        }
    }
}
impl Transport for Fake {
    fn resolve(&self, host: &str, _: Duration) -> Result<Vec<IpAddr>, String> {
        self.calls.borrow_mut().push(format!("dns:{host}"));
        Ok(self.addresses.clone())
    }
    fn get(&self, url: &Url, addresses: &[IpAddr], _: Duration) -> Result<Response, String> {
        assert!(!addresses.is_empty());
        self.calls.borrow_mut().push(url.to_string());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or("unexpected HTTP".into())
    }
}
fn response(body: &[u8]) -> Response {
    Response {
        status: 200,
        location: None,
        content_type: Some("text/plain; charset=UTF-8".into()),
        content_encoding: None,
        content_length: None,
        body: Box::new(Cursor::new(body.to_vec())),
    }
}
fn redirect(location: &str) -> Response {
    Response {
        status: 302,
        location: Some(location.into()),
        ..response(b"ignored redirect body")
    }
}
fn limits() -> Limits {
    Limits {
        max_bytes: 1024,
        max_redirects: 3,
        max_duration: Duration::from_secs(1),
    }
}
fn fetch(fake: &Fake, url: &str) -> Result<PageSnapshot, String> {
    acquire(fake, url, &limits(), &mut || Ok(()))
}
#[test]
fn retains_exact_source_bytes_and_redirect_provenance() {
    let text = "Имя: Аня 🦉\r\n  слово\t\n";
    let fake = Fake::new(vec![redirect("/final#section"), response(text.as_bytes())]);
    let got = fetch(&fake, "https://public.example/start#intro").unwrap();
    assert_eq!(got.original, text);
    assert_eq!(got.selected_url, "https://public.example/start#intro");
    assert_eq!(got.final_url, "https://public.example/final");
    assert_eq!(got.redirects, vec!["https://public.example/final"]);
    use sha2::{Digest, Sha256};
    assert_eq!(got.sha256, format!("{:x}", Sha256::digest(text.as_bytes())));
    assert_eq!(got.content_type, "text/plain");
    assert!(chrono::DateTime::parse_from_rfc3339(&got.retrieved_at).is_ok());
}
#[test]
fn forbidden_urls_fail_before_dns_or_http() {
    for url in [
        "file:///etc/passwd",
        "ftp://public.example/a",
        "https://name:pass@public.example",
        "https://public.example:8443",
        "http://127.0.0.1",
        "http://2130706433",
        "http://[::1]",
        "http://169.254.169.254/latest",
        "http://localhost",
        "http://localhost.",
    ] {
        let fake = Fake::new(vec![response(b"secret")]);
        assert!(fetch(&fake, url).is_err(), "{url}");
        assert!(fake.calls.borrow().is_empty(), "{url}");
    }
}
#[test]
fn mixed_dns_and_special_ip_ranges_fail_before_http() {
    for address in [
        "0.0.0.0",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.2",
        "169.254.1.1",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.0.9",
        "192.0.2.1",
        "192.88.99.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:127.0.0.1",
        "64:ff9b::7f00:1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "2001::1",
        "3fff::1",
    ] {
        let mut fake = Fake::new(vec![response(b"secret")]);
        fake.addresses.push(address.parse().unwrap());
        assert!(fetch(&fake, "https://public.example").is_err(), "{address}");
        assert_eq!(fake.calls.borrow().len(), 1, "{address}");
    }
}
#[test]
fn redirect_targets_are_revalidated_and_loops_are_bounded() {
    for target in [
        "http://127.0.0.1/admin",
        "http://public.example/cleartext",
        "file:///etc/passwd",
        "https://user:pass@public.example/",
    ] {
        let fake = Fake::new(vec![redirect(target), response(b"secret")]);
        assert!(fetch(&fake, "https://public.example/").is_err(), "{target}");
        assert_eq!(fake.calls.borrow().len(), 2);
    }
    let fake = Fake::new(vec![
        redirect("/b"),
        redirect("/c"),
        redirect("/d"),
        redirect("/e"),
        response(b"too far"),
    ]);
    assert!(fetch(&fake, "https://public.example/a").is_err());
    assert_eq!(fake.calls.borrow().len(), 8);
}
#[test]
fn rejects_oversize_even_without_or_with_false_content_length() {
    for length in [None, Some(1), Some(2048)] {
        let fake = Fake::new(vec![Response {
            content_length: length,
            ..response(&vec![b'a'; 1025])
        }]);
        assert!(fetch(&fake, "https://public.example/").is_err());
    }
}
#[test]
fn unsupported_types_encodings_partial_and_error_responses_are_not_snapshots() {
    for content_type in [
        None,
        Some("application/json"),
        Some("application/octet-stream"),
        Some("text/plain; charset=windows-1251"),
    ] {
        let fake = Fake::new(vec![Response {
            content_type: content_type.map(str::to_owned),
            ..response(b"text")
        }]);
        assert!(fetch(&fake, "https://public.example").is_err());
    }
    for status in [204, 206, 304, 401, 404, 500] {
        assert!(fetch(
            &Fake::new(vec![Response {
                status,
                ..response(b"text")
            }]),
            "https://public.example"
        )
        .is_err());
    }
    assert!(fetch(
        &Fake::new(vec![Response {
            content_encoding: Some("gzip".into()),
            ..response(b"text")
        }]),
        "https://public.example"
    )
    .is_err());
    assert!(fetch(
        &Fake::new(vec![response(&[0xff, 0xfe])]),
        "https://public.example"
    )
    .is_err());
}
#[test]
fn cancellation_and_invalid_budgets_prevent_any_transport_access() {
    let fake = Fake::new(vec![]);
    assert_eq!(
        acquire(&fake, "https://public.example", &limits(), &mut || Err(
            "cancelled".into()
        ))
        .err()
        .unwrap(),
        "cancelled"
    );
    for value in [
        Limits {
            max_bytes: 0,
            ..limits()
        },
        Limits {
            max_bytes: 17 * 1024 * 1024,
            ..limits()
        },
        Limits {
            max_redirects: 6,
            ..limits()
        },
        Limits {
            max_duration: Duration::ZERO,
            ..limits()
        },
    ] {
        assert!(acquire(&fake, "https://public.example", &value, &mut || Ok(())).is_err());
    }
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn checks_cancellation_after_dns_and_after_body_io() {
    let fake = Fake::new(vec![response(b"source")]);
    let mut checks = 0;
    assert_eq!(
        acquire(&fake, "https://public.example", &limits(), &mut || {
            checks += 1;
            if checks == 3 {
                Err("cancelled after DNS".into())
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap(),
        "cancelled after DNS"
    );
    assert_eq!(fake.calls.borrow().len(), 1);
    struct CancellingRead(std::rc::Rc<std::cell::Cell<bool>>);
    impl Read for CancellingRead {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.0.set(true);
            bytes[0] = b'x';
            Ok(1)
        }
    }
    let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
    let fake = Fake::new(vec![Response {
        body: Box::new(CancellingRead(cancelled.clone())),
        ..response(b"")
    }]);
    assert_eq!(
        acquire(&fake, "https://public.example", &limits(), &mut || {
            if cancelled.get() {
                Err("cancelled during read".into())
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap(),
        "cancelled during read"
    );
}
#[test]
fn empty_excessive_and_rebound_dns_never_reaches_second_http_request() {
    for addresses in [vec![], vec!["93.184.215.14".parse().unwrap(); 33]] {
        let fake = Fake {
            addresses,
            ..Fake::new(vec![response(b"source")])
        };
        assert!(fetch(&fake, "https://public.example").is_err());
        assert_eq!(fake.calls.borrow().len(), 1);
    }
    struct Rebinding(Fake);
    impl Transport for Rebinding {
        fn resolve(&self, host: &str, timeout: Duration) -> Result<Vec<IpAddr>, String> {
            if self.0.calls.borrow().len() == 2 {
                Ok(vec!["127.0.0.1".parse().unwrap()])
            } else {
                self.0.resolve(host, timeout)
            }
        }
        fn get(
            &self,
            url: &Url,
            addresses: &[IpAddr],
            timeout: Duration,
        ) -> Result<Response, String> {
            self.0.get(url, addresses, timeout)
        }
    }
    let transport = Rebinding(Fake::new(vec![redirect("/b"), response(b"secret")]));
    assert!(acquire(
        &transport,
        "https://public.example/a",
        &limits(),
        &mut || Ok(())
    )
    .is_err());
    assert_eq!(transport.0.calls.borrow().len(), 2);
}
#[test]
fn public_literals_are_checked_without_dns_and_real_adapter_rejects_private_urls() {
    for url in ["http://93.184.215.14/", "https://[2606:4700:4700::1111]/"] {
        let fake = Fake::new(vec![response(b"source")]);
        assert!(fetch(&fake, url).is_ok());
        assert_eq!(*fake.calls.borrow(), [url]);
    }
    assert!(super::fetch("http://127.0.0.1/", &limits(), &mut || Ok(())).is_err());
}
#[test]
fn overall_deadline_includes_resolution_and_response_reading() {
    struct Slow(Fake);
    impl Transport for Slow {
        fn resolve(&self, host: &str, timeout: Duration) -> Result<Vec<IpAddr>, String> {
            std::thread::sleep(Duration::from_millis(10));
            self.0.resolve(host, timeout)
        }
        fn get(&self, _: &Url, _: &[IpAddr], _: Duration) -> Result<Response, String> {
            panic!("expired DNS must not reach HTTP")
        }
    }
    assert!(acquire(
        &Slow(Fake::new(vec![])),
        "https://public.example",
        &Limits {
            max_duration: Duration::from_millis(1),
            ..limits()
        },
        &mut || Ok(())
    )
    .is_err());
    struct SlowBody;
    impl Read for SlowBody {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(10));
            Ok(0)
        }
    }
    let fake = Fake::new(vec![Response {
        body: Box::new(SlowBody),
        ..response(b"")
    }]);
    assert!(acquire(
        &fake,
        "https://public.example",
        &Limits {
            max_duration: Duration::from_millis(1),
            ..limits()
        },
        &mut || Ok(())
    )
    .is_err());
}
