use super::*;
use std::io::BufRead;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    pub capture: Fixture,
    pub slot: u8,
}

pub(super) fn verify(pair: &Pair, base: &Path, replay: &Value, input_hash: &str) -> Result<()> {
    let waits = replay["resource_waits"]
        .as_array()
        .filter(|v| !v.is_empty());
    let Some(source) = &pair.resource_wait_source else {
        ensure!(
            waits.is_none(),
            "resource waits require independent source observations"
        );
        return Ok(());
    };
    let waits = waits.context("resource observation source has no replay waits")?;
    ensure!(source.slot < 32, "invalid observed VM slot");
    let file = source.capture.verify(base)?;
    let capture: Value = serde_json::from_slice(&fs::read(&file)?)?;
    verify_capture(pair, &capture, input_hash)?;
    let memory = file.parent().unwrap().join("memory.jsonl");
    check_hash(
        &memory,
        capture["memory_watch"]["sha256"]
            .as_str()
            .context("missing observation hash")?,
    )?;
    let advances: BTreeMap<u32, u32> = serde_json::from_value(
        replay
            .get("presentation_advances")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )?;
    let source_vi = |tick: u64| -> Result<u32> {
        let tick = u32::try_from(tick)?;
        advances.range(..=tick).try_fold(tick, |vi, (_, extra)| {
            vi.checked_add(*extra).context("VI overflow")
        })
    };
    let intervals: Vec<_> = waits
        .iter()
        .map(|wait| {
            Ok((
                source_vi(
                    wait["request_tick"]
                        .as_u64()
                        .context("missing resource request tick")?,
                )?,
                source_vi(
                    wait["resume_tick"]
                        .as_u64()
                        .context("missing resource resume tick")?,
                )?,
                wait["pc"].as_u64().context("missing resource wait PC")?,
            ))
        })
        .collect::<Result<_>>()?;
    let mut rows = BTreeMap::new();
    for line in std::io::BufReader::new(fs::File::open(memory)?).lines() {
        let row: Value = serde_json::from_str(&line?)?;
        let vi = u32::try_from(
            row["vi_sample"]
                .as_u64()
                .context("missing observation VI")?,
        )?;
        if intervals
            .iter()
            .any(|&(start, end, _)| (start..=end).contains(&vi))
        {
            ensure!(
                rows.insert(vi, row).is_none(),
                "duplicate resource observation VI"
            );
        }
    }
    let key = |name: &str| format!("vm_{}_{name}", source.slot);
    for (start, end, pc) in intervals {
        ensure!(start < end, "invalid resource observation interval");
        let first = rows
            .get(&start)
            .context("missing resource request observation")?;
        let handle = first[&key("wait_value")]
            .as_u64()
            .context("missing resource handle")?;
        ensure!(
            handle & 0xffff0000 == 0xffff0000,
            "invalid source resource handle"
        );
        for vi in start..end {
            let row = rows.get(&vi).context("missing resource wait observation")?;
            let cleared = vi == end - 1;
            ensure!(
                row[&key("pc")] == pc
                    && row[&key("program")] == first[&key("program")]
                    && row[&key("wait_mode")] == u64::from(!cleared)
                    && row[&key("wait_value")] == if cleared { 0 } else { handle },
                "resource wait differs from source VM at VI {vi}"
            );
        }
        ensure!(
            rows.get(&end)
                .context("missing resource continuation observation")?[&key("pc")]
                != pc,
            "source VM did not resume after resource readiness"
        );
    }
    Ok(())
}
