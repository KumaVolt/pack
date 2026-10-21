//! A tiny DNS server: every `*.<tld>` name resolves to 127.0.0.1.
//! The OS sends `.test` lookups here (see `platform::privileged_script`).

use std::net::Ipv4Addr;
use tokio::net::UdpSocket;

pub async fn serve(port: u16, tld: String) -> anyhow::Result<()> {
    let socket = UdpSocket::bind(("127.0.0.1", port)).await?;
    let mut buf = [0u8; 512];
    loop {
        let Ok((len, peer)) = socket.recv_from(&mut buf).await else { continue };
        if let Some(reply) = answer(&buf[..len], &tld) {
            let _ = socket.send_to(&reply, peer).await;
        }
    }
}

/// Build a response to a single-question query. Names under `tld` get an A
/// record for 127.0.0.1; everything else gets NXDOMAIN.
pub fn answer(query: &[u8], tld: &str) -> Option<Vec<u8>> {
    if query.len() < 12 || u16::from_be_bytes([query[4], query[5]]) != 1 {
        return None;
    }
    // Parse the question name.
    let mut pos = 12;
    let mut labels = vec![];
    loop {
        let len = *query.get(pos)? as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        labels.push(String::from_utf8_lossy(query.get(pos..pos + len)?).to_ascii_lowercase());
        pos += len;
    }
    let qtype = u16::from_be_bytes([*query.get(pos)?, *query.get(pos + 1)?]);
    let question_end = pos + 4;
    query.get(..question_end)?;

    let ours = labels.last().is_some_and(|l| l == tld);
    let a_record = ours && (qtype == 1 || qtype == 255);

    let mut out = Vec::with_capacity(question_end + 16);
    out.extend_from_slice(&query[0..2]); // id
    let rd = query[2] & 0x01;
    out.push(0x84 | rd); // QR, AA, RD copied
    out.push(if ours { 0x00 } else { 0x03 }); // NOERROR / NXDOMAIN
    out.extend_from_slice(&[0, 1]); // qdcount
    out.extend_from_slice(&[0, a_record as u8]); // ancount
    out.extend_from_slice(&[0, 0, 0, 0]); // nscount, arcount
    out.extend_from_slice(&query[12..question_end]);
    if a_record {
        out.extend_from_slice(&[0xC0, 0x0C]); // pointer to the question name
        out.extend_from_slice(&[0, 1, 0, 1]); // A, IN
        out.extend_from_slice(&60u32.to_be_bytes()); // TTL
        out.extend_from_slice(&[0, 4]);
        out.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn resolves_tld_to_localhost() {
        let reply = answer(&query("shop.Laravel.test", 1), "test").unwrap();
        assert_eq!(&reply[0..2], &[0x12, 0x34]);
        assert_eq!(reply[3] & 0x0F, 0);
        assert_eq!(reply[7], 1);
        assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);
    }

    #[test]
    fn aaaa_is_empty_noerror() {
        let reply = answer(&query("app.test", 28), "test").unwrap();
        assert_eq!(reply[3] & 0x0F, 0);
        assert_eq!(reply[7], 0);
    }

    #[test]
    fn other_tlds_are_nxdomain() {
        let reply = answer(&query("example.com", 1), "test").unwrap();
        assert_eq!(reply[3] & 0x0F, 3);
    }
}
