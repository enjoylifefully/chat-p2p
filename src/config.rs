use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use iroh::{NodeAddr, NodeId, SecretKey};
use palette::named::FIREBRICK;

pub fn key_path() -> PathBuf {
    let mut home = dirs::home_dir().expect("HOME não encontrado");
    home.push(".liboo");
    std::fs::create_dir_all(&home).expect("não deu pra criar ~/.liboo");
    home.push("key");
    home
}

pub fn friends_path() -> PathBuf {
    let mut home = dirs::home_dir().expect("HOME não encontrado");
    home.push(".liboo");
    std::fs::create_dir_all(&home).expect("não deu pra criar ~/.liboo");
    home.push("friends");
    home
}

pub fn load_salt() -> Result<[u8; 32]> {
    let path = key_path();
    if !path.exists() {
        let salt: [u8; 32] = rand::random();
        let mut enconded = base58::encode(salt).into_string();

        enconded.push('\n');
        std::fs::write(path, enconded)?;

        Ok(salt)
    } else {
        let file = std::fs::read_to_string(path)?;
        let encoded = file.trim().as_bytes();
        let decoded = base58::decode(encoded).into_array_const()?;

        Ok(decoded)
    }
}

pub fn generate_secret_key(name: &str) -> Result<SecretKey> {
    let salt = load_salt()?;
    let hash = blake3::Hasher::new()
        .update(name.as_bytes())
        .update(&salt)
        .finalize();

    Ok(SecretKey::from_bytes(hash.as_bytes()))
}

pub fn load_friends() -> Result<BTreeSet<NodeId>> {
    let path = friends_path();

    let mut set = BTreeSet::new();

    if !path.exists() {
        return Ok(BTreeSet::new());
    }

    let existing = fs::read_to_string(&path)?;
    for line in existing.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let encoded = line.as_bytes();
        let decoded = base58::decode(encoded).into_array_const()?;

        set.insert(NodeId::from_bytes(&decoded)?);
    }

    Ok(set)
}

pub fn load_friends_without_me(me: NodeId) -> Result<Vec<NodeId>> {
    let mut friends = load_friends()?;

    friends.remove(&me);

    Ok(friends.into_iter().collect())
}

pub fn add_friends(new_friends_raw: &Vec<String>) -> Result<()> {
    let path = friends_path();

    let mut friends = load_friends()?;

    for s in new_friends_raw {
        let s = s.trim();
        if !s.is_empty() {
            let encoded = s.as_bytes();
            let decoded = base58::decode(encoded).into_array_const()?;

            friends.insert(NodeId::from_bytes(&decoded)?);
        }
    }

    let buf: String = friends
        .into_iter()
        .map(|node| {
            let mut line = base58::encode(node).into_string();
            line.push('\n');
            line
        })
        .collect();

    fs::write(&path, buf)?;

    Ok(())
}
