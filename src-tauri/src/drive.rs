//! Google Drive OAuth and uploads. Desktop OAuth uses PKCE and a loopback
//! redirect; refresh credentials exist only in macOS Keychain.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Runtime};

const CLIENT_ID: &str = "59686342197-5a58rrnmoafohg1dn7voj7is5ssfu3ec.apps.googleusercontent.com";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
/// The specific Google Drive folder where screenshots are uploaded.
const FOLDER_ID: &str = "1CTZOgxCOkMxwwvo-Jqb5TEh3NxTl8IJi";

fn client() -> reqwest::Client {
    crate::update::ensure_crypto_provider();
    reqwest::Client::new()
}

fn entry<R: Runtime>(app: &AppHandle<R>) -> Result<keyring::Entry, String> {
    let service = format!("{}.google-drive", app.config().identifier);
    keyring::Entry::new(&service, "refresh-token").map_err(|e| e.to_string())
}
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn valid_drive_link(s: &str) -> bool {
    url::Url::parse(s)
        .ok()
        .is_some_and(|u| u.scheme() == "https" && u.host_str() == Some("drive.google.com"))
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}
#[derive(Deserialize)]
struct FileInfo {
    id: String,
    web_view_link: Option<String>,
}

#[tauri::command]
pub fn drive_connected<R: Runtime>(app: AppHandle<R>) -> bool {
    entry(&app)
        .and_then(|e| e.get_password().map_err(|e| e.to_string()))
        .is_ok()
}
#[tauri::command]
pub async fn drive_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let saved = entry(&app)?;
    let token = saved.get_password().ok();
    match saved.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.to_string()),
    }
    if let Some(token) = token {
        // Local disconnect takes effect even when offline; revoke remotely
        // on a best-effort basis after removing the credential locally.
        let _ = client()
            .post("https://oauth2.googleapis.com/revoke")
            .form(&[("token", token)])
            .send()
            .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn drive_connect<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let state = uuid::Uuid::new_v4().to_string();
    let mut auth = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    auth.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);
    let auth_url = auth.to_string();
    let callback = tokio::task::spawn_blocking(move || -> Result<String, String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut stream: TcpStream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Err("Google authorization timed out".into())
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(180)))
            .map_err(|e| e.to_string())?;
        let mut b = [0u8; 8192];
        let n = stream.read(&mut b).map_err(|e| e.to_string())?;
        let line = String::from_utf8_lossy(&b[..n])
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let target = line
            .split_whitespace()
            .nth(1)
            .ok_or("Invalid OAuth callback")?;
        let cb = url::Url::parse(&format!("http://127.0.0.1:{port}{target}"))
            .map_err(|e| e.to_string())?;
        let p: std::collections::HashMap<_, _> = cb.query_pairs().into_owned().collect();
        let html = "<!DOCTYPE html><html><body style=\"font-family:system-ui,-apple-system,sans-serif;text-align:center;padding:60px 20px;background:#1c1c1e;color:#f2f2f7;\"><h2 style=\"color:#30d158;\">&#10003; Google Drive Connected</h2><p>You can close this tab and return to Socorin.</p></body></html>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        if p.get("state") != Some(&state) {
            return Err("Google OAuth state validation failed".into());
        }
        if let Some(e) = p.get("error") {
            return Err(format!("Google authorization failed: {e}"));
        }
        p.get("code")
            .cloned()
            .ok_or("Google did not return an authorization code".into())
    });
    tauri_plugin_opener::open_url(&auth_url, None::<&str>).map_err(|e| e.to_string())?;
    let code = callback.await.map_err(|e| e.to_string())??;
    let response: Token = client()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("code_verifier", &verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &redirect),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| format!("Token exchange failed: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let refresh=response.refresh_token.ok_or("Google did not provide a refresh token; disconnect Socorin in your Google Account settings and reconnect")?;
    entry(&app)?
        .set_password(&refresh)
        .map_err(|e| format!("Could not save refresh token to macOS Keychain: {e}"))?;
    crate::windows::show_settings(&app);
    Ok(())
}

async fn access_token<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    let refresh = entry(app)?
        .get_password()
        .map_err(|_| "Connect Google Drive in Socorin Settings first".to_string())?;
    let t: Token = client()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("refresh_token", &refresh),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| format!("Google token refresh failed: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let _ = t.expires_in;
    Ok(t.access_token)
}

pub async fn upload<R: Runtime>(
    app: &AppHandle<R>,
    bytes: Vec<u8>,
    filename: &str,
    mime: &str,
) -> Result<String, String> {
    let token = access_token(app).await?;
    let boundary = format!("socorin_{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{{\"name\":{},\"parents\":[\"{FOLDER_ID}\"]}}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n",serde_json::to_string(filename).unwrap()).as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
    let client = client();
    let file:FileInfo=client.post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,webViewLink")
        .bearer_auth(&token).header("Content-Type",format!("multipart/related; boundary={boundary}"))
        .body(body).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|format!("Google Drive upload failed: {e}"))?.json().await.map_err(|e|e.to_string())?;
    let perm = client
        .post(format!(
            "https://www.googleapis.com/drive/v3/files/{}/permissions?fields=id",
            file.id
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"type":"anyone","role":"reader"}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !perm.status().is_success() {
        return Err(format!(
            "File uploaded, but Google would not enable link viewing (HTTP {})",
            perm.status()
        ));
    }
    let link = file
        .web_view_link
        .ok_or("Google Drive did not return a share link")?;
    if !valid_drive_link(&link) {
        return Err("Google Drive returned an unexpected link".into());
    }
    Ok(link)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_uses_s256_base64url() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn only_https_drive_links_are_accepted() {
        assert!(valid_drive_link("https://drive.google.com/file/d/abc/view"));
        assert!(!valid_drive_link("https://socorin.com/a"));
        assert!(!valid_drive_link("javascript:alert(1)"));
    }
}
