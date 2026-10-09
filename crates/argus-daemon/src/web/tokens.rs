//! Tokens (§5: "Any remote access — the tablet review client in §11, or a
//! teammate's editor — goes through a token issued by `argus token` with a
//! scope (observe, review, edit) and an expiry"; phase 5 item 10).
//!
//! `.argus/tokens.json` keeps each token's id, label, scope, when it was
//! made and when it expires, and the SHA-1 of the token, never the token:
//! it is shown once, when issued. A token is 20 random bytes from the
//! operating system, as hex.

use super::hash::{hex, sha1};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

/// What a token lets its holder do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Look: every read, frames, subscriptions.
    Observe,
    /// Look, and answer review requests, drop and note marks.
    Review,
    /// What the editor does, but for the daemon's own (tokens, opening
    /// another project, stopping the daemon, a window on this machine).
    Edit,
}

impl Scope {
    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Scope::Observe => "observe",
            Scope::Review => "review",
            Scope::Edit => "edit",
        }
    }

    /// The scope a name names.
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "observe" => Some(Scope::Observe),
            "review" => Some(Scope::Review),
            "edit" => Some(Scope::Edit),
            _ => None,
        }
    }
}

/// One token as kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// A short handle (the hash's first eight characters), for list and
    /// revoke.
    pub id: String,
    /// What it is for (`phone`).
    pub label: String,
    /// What it allows.
    pub scope: Scope,
    /// Seconds since 1970.
    pub made_at: u64,
    /// Seconds since 1970.
    pub expires_at: u64,
    /// SHA-1 of the token, hex.
    pub hash: String,
}

impl Token {
    fn to_json(&self) -> Json {
        json!({
            "id": self.id, "label": self.label, "scope": self.scope.name(),
            "made_at": self.made_at, "expires_at": self.expires_at, "hash": self.hash,
        })
    }

    fn from_json(j: &Json) -> Option<Token> {
        Some(Token {
            id: j["id"].as_str()?.to_string(),
            label: j["label"].as_str().unwrap_or("").to_string(),
            scope: Scope::parse(j["scope"].as_str()?)?,
            made_at: j["made_at"].as_u64().unwrap_or(0),
            expires_at: j["expires_at"].as_u64()?,
            hash: j["hash"].as_str()?.to_string(),
        })
    }

    /// For `token.list`: everything but the hash.
    pub fn row(&self, now: u64) -> Json {
        json!({
            "id": self.id, "label": self.label, "scope": self.scope.name(),
            "made_at": self.made_at, "expires_at": self.expires_at,
            "expired": now >= self.expires_at,
        })
    }
}

/// The project's tokens file.
pub fn file(root: &Path) -> PathBuf {
    root.join(".argus").join("tokens.json")
}

fn bad(msg: String) -> ArgusError {
    ArgusError::new(crate::codes::BAD_PARAMS, msg)
}

/// Every token kept (expired ones too, until revoked).
pub fn load(root: &Path) -> Vec<Token> {
    let Ok(text) = std::fs::read_to_string(file(root)) else {
        return Vec::new();
    };
    let Ok(j) = serde_json::from_str::<Json>(&text) else {
        return Vec::new();
    };
    j["tokens"]
        .as_array()
        .map(|a| a.iter().filter_map(Token::from_json).collect())
        .unwrap_or_default()
}

fn save(root: &Path, tokens: &[Token]) -> Result<()> {
    let p = file(root);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)
            .map_err(|e| ArgusError::new(1, format!("{}: {e}", d.display())))?;
    }
    let j = json!({ "tokens": tokens.iter().map(Token::to_json).collect::<Vec<_>>() });
    std::fs::write(&p, serde_json::to_string_pretty(&j).unwrap_or_default())
        .map_err(|e| ArgusError::new(1, format!("{}: {e}", p.display())))
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Make a token: the token itself (shown once) and what is kept.
pub fn issue(root: &Path, scope: Scope, hours: f64, label: &str) -> Result<(String, Token)> {
    if !(hours > 0.0 && hours <= 24.0 * 30.0) {
        return Err(bad(format!(
            "hours {hours}: a token lasts more than 0 hours and at most 30 days (720)"
        )));
    }
    let mut raw = [0u8; 20];
    getrandom::getrandom(&mut raw)
        .map_err(|e| ArgusError::new(1, format!("no randomness from the system: {e}")))?;
    let secret = format!("argus-{}", hex(&raw));
    let hash = hex(&sha1(secret.as_bytes()));
    let made = now();
    let t = Token {
        id: hash[..8].to_string(),
        label: label.to_string(),
        scope,
        made_at: made,
        expires_at: made + (hours * 3600.0) as u64,
        hash,
    };
    let mut all = load(root);
    all.push(t.clone());
    save(root, &all)?;
    Ok((secret, t))
}

/// Forget a token by id; whether there was one.
pub fn revoke(root: &Path, id: &str) -> Result<bool> {
    let mut all = load(root);
    let before = all.len();
    all.retain(|t| t.id != id);
    let gone = all.len() != before;
    if gone {
        save(root, &all)?;
    }
    Ok(gone)
}

/// The token a secret is, if it is one and has not expired.
pub fn check(root: &Path, secret: &str) -> Option<Token> {
    let hash = hex(&sha1(secret.trim().as_bytes()));
    let t = now();
    load(root)
        .into_iter()
        .find(|k| k.hash == hash && t < k.expires_at)
}
