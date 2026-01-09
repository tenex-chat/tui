//! Blossom blob upload for images
//!
//! Blossom is a protocol for storing blobs on Nostr.
//! See: https://github.com/hzrd149/blossom

use nostr_sdk::{Keys, Kind, TagKind, EventBuilder, Tag, Timestamp};
use reqwest::Client;
use sha2::{Sha256, Digest};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const BLOSSOM_SERVER: &str = "https://blossom.primal.net";

/// Upload an image to Blossom and return the URL
pub async fn upload_image(data: &[u8], keys: &Keys, mime_type: &str) -> anyhow::Result<String> {
    // Calculate SHA-256 hash of the blob
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    // Create authorization event (kind 24242)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let expiration = now + 300; // 5 minutes

    let auth_event = EventBuilder::new(Kind::Custom(24242), "Upload")
        .tag(Tag::custom(TagKind::custom("t"), ["upload"]))
        .tag(Tag::custom(TagKind::custom("x"), [&hash_hex]))
        .tag(Tag::expiration(Timestamp::from(expiration)))
        .sign_with_keys(keys)?;

    // Base64 encode the authorization event
    let auth_json = serde_json::to_string(&auth_event)?;
    let auth_base64 = base64_encode(&auth_json);

    // Upload to Blossom
    let client = Client::new();
    let response = client
        .put(format!("{}/upload", BLOSSOM_SERVER))
        .header("Authorization", format!("Nostr {}", auth_base64))
        .header("Content-Type", mime_type)
        .body(data.to_vec())
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Blossom upload failed: {} - {}", status, body);
    }

    // Parse response to get URL
    let blob_descriptor: BlobDescriptor = response.json().await?;
    Ok(blob_descriptor.url)
}

#[derive(serde::Deserialize)]
struct BlobDescriptor {
    url: String,
}

fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).ok();
    }
    String::from_utf8(buf).unwrap_or_default()
}

// Simple base64 encoder
struct Base64Encoder<'a> {
    output: &'a mut Vec<u8>,
    buffer: u32,
    bits: u8,
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> Base64Encoder<'a> {
    fn new(output: &'a mut Vec<u8>) -> Self {
        Self {
            output,
            buffer: 0,
            bits: 0,
        }
    }

    fn flush_buffer(&mut self) {
        while self.bits >= 6 {
            self.bits -= 6;
            let idx = ((self.buffer >> self.bits) & 0x3F) as usize;
            self.output.push(BASE64_CHARS[idx]);
        }
    }
}

impl<'a> std::io::Write for Base64Encoder<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &byte in buf {
            self.buffer = (self.buffer << 8) | (byte as u32);
            self.bits += 8;
            self.flush_buffer();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.bits > 0 {
            self.buffer <<= 6 - self.bits;
            self.bits = 6;
            self.flush_buffer();
        }
        // Add padding
        while self.output.len() % 4 != 0 {
            self.output.push(b'=');
        }
        Ok(())
    }
}

impl<'a> Drop for Base64Encoder<'a> {
    fn drop(&mut self) {
        self.flush().ok();
    }
}
