use super::*;
use crate::dol;
use std::collections::BTreeMap;

const MOVIES: u32 = 0x801f9c80;

pub(crate) fn directory(executable: &[u8]) -> Result<Vec<Option<String>>> {
    dol::slice(executable, MOVIES, 19 * 4)?
        .chunks_exact(4)
        .map(|row| dol::optional_text(executable, be_u32(row, 0)?))
        .collect()
}

pub(crate) fn cook_directory(executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let path = "embedded/movies.json";
    write_json(&output.join(path), &json!(directory(executable)?))?;
    Ok(vec![path.into()])
}

/// Resolve original machine IDs before publication. Repeated declarations must
/// select the same movie; source media do not introduce runtime namespaces.
pub(crate) fn catalogue(discs: &[PathBuf]) -> Result<BTreeMap<u32, (u8, String)>> {
    let mut sources = discs
        .iter()
        .map(|path| Ok((crate::disc_number(path)?, path)))
        .collect::<Result<Vec<_>>>()?;
    sources.sort_by_key(|(disc, _)| *disc);
    let mut declarations = BTreeMap::<u32, (u8, String)>::new();
    for (disc, extracted) in sources {
        for (id, path) in directory(&fs::read(extracted.join("sys/main.dol"))?)?
            .into_iter()
            .enumerate()
        {
            if let Some(path) = path
                && let Some(path) =
                    crate::field_resources::find_path(&extracted.join("files"), &path)?
            {
                if let Some((previous_disc, previous)) = declarations.get(&(id as u32)) {
                    ensure!(
                        previous.eq_ignore_ascii_case(&path),
                        "conflicting movie {id} source bindings on discs {previous_disc} and {disc}: {previous} versus {path}"
                    );
                } else {
                    declarations.insert(id as u32, (disc, path));
                }
            }
        }
    }
    Ok(declarations)
}
