//! Compose Hunspell, user, and project dictionaries into one suggester source.

use std::error::Error;

use ferrolex::{
    import, Checker, Dictionary, ImportMode, Normalization, SuggestConfig, Suggester,
    UserDictionary, WordList,
};

fn main() -> Result<(), Box<dyn Error>> {
    let hunspell = import(
        "example.aff",
        "SET UTF-8\n",
        "example.dic",
        "1\nferrolex\n",
        ImportMode::Strict,
    )?
    .into_dictionary();
    let user = UserDictionary::from_text(Normalization::Nfc, "workspace-term\n");
    let project = WordList::new(["project-term"])?;
    let checker = Checker::builder()
        .dictionary(hunspell)
        .dictionary(user)
        .dictionary(project)
        .build();

    assert!(checker.contains("ferrolex"));
    assert!(checker.contains("workspace-term"));
    assert!(checker.contains("project-term"));
    let suggestions = Suggester::new(&checker, SuggestConfig::default()).suggest("ferolex");
    println!("suggestions: {:?}", suggestions.suggestions());
    Ok(())
}
