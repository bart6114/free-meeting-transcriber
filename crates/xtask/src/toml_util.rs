use anyhow::{Context, Result};
use std::{fs, path::Path};

type TomlTable = toml::map::Map<String, toml::Value>;

pub(crate) fn toml_set(args: &[String]) -> Result<()> {
    let (path_str, pairs) = args
        .split_first()
        .context("usage: toml-set <file> <key> <value> ...")?;
    anyhow::ensure!(pairs.len() % 2 == 0, "key/value args must come in pairs");

    let path = Path::new(path_str);
    let mut config: toml::Table = fs::read_to_string(path)
        .with_context(|| format!("read {path_str}"))?
        .parse()
        .with_context(|| format!("parse {path_str}"))?;

    for chunk in pairs.chunks_exact(2) {
        let (key, val_str) = (&chunk[0], &chunk[1]);
        let value = format!("x={val_str}")
            .parse::<toml::Table>()
            .with_context(|| format!("invalid TOML value: {val_str}"))?
            .remove("x")
            .unwrap();
        toml_set_key(&mut config, key, value);
    }

    fs::write(
        path,
        toml::to_string_pretty(&config).context("serialize TOML")?,
    )
    .with_context(|| format!("write {path_str}"))
}

fn toml_set_key(table: &mut TomlTable, key: &str, value: toml::Value) {
    match key.split_once('.') {
        None => {
            table.insert(key.to_string(), value);
        }
        Some((k, rest)) => {
            let sub = table
                .entry(k)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let toml::Value::Table(t) = sub {
                toml_set_key(t, rest, value);
            }
        }
    }
}
