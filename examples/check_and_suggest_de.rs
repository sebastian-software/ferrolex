//! Install the reviewed German catalog entry, import it, check a word, and suggest a correction.

use std::error::Error;
use std::fs;

use ferrolex::{
    catalog_import_encodings, find_locale, import_bytes, import_bytes_with_encodings, Dictionary,
    DictionaryInstaller, ImportMode, SuggestConfig, UreqFetcher,
};

fn main() -> Result<(), Box<dyn Error>> {
    let source = find_locale("de_DE").ok_or("de_DE is not in the reviewed catalog")?;
    let manifest = source.manifest()?;
    let cache_root =
        std::env::temp_dir().join(format!("ferrolex-check-and-suggest-{}", std::process::id()));

    let result = (|| -> Result<(), Box<dyn Error>> {
        let installed = DictionaryInstaller::new(UreqFetcher).install(&manifest, &cache_root)?;
        let aff = fs::read(installed.aff_path())?;
        let dic = fs::read(installed.dic_path())?;
        let aff_name = installed.aff_path().to_string_lossy();
        let dic_name = installed.dic_path().to_string_lossy();
        let imported = match catalog_import_encodings(source.encoding()) {
            Some(encodings) => import_bytes_with_encodings(
                &aff_name,
                &aff,
                &dic_name,
                &dic,
                encodings,
                ImportMode::Strict,
            )?,
            None => import_bytes(&aff_name, &aff, &dic_name, &dic, ImportMode::Strict)?,
        };
        let dictionary = imported.into_dictionary();

        println!("Haus recognized: {}", dictionary.contains("Haus"));
        let suggestions = dictionary
            .suggester(SuggestConfig::default())
            .suggest("Hause");
        for suggestion in suggestions.suggestions() {
            println!("suggestion: {}", suggestion.word());
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&cache_root);
    result
}
