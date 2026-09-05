//! Optional, verified acquisition of third-party dictionaries.
//!
//! This crate deliberately has no automatic update loop and no bundled
//! dictionary data. Callers choose the cache directory and select a reviewed
//! [`VerifiedDictionary`] manifest. Bytes are accepted only from HTTPS URLs,
//! checked against the manifest's SHA-256 digests, and atomically installed.
//!
//! ```
//! use ferrolex_dictionaries::find_locale;
//!
//! assert_eq!(find_locale("en_US").expect("catalogued locale").locale(), "en_US");
//! ```
//!
//! An installer receives an explicit cache root and a fetcher, so acquisition
//! can be tested without a network and the cache location never comes from
//! ambient process state:
//!
//! ```
//! use ferrolex_dictionaries::{
//!     DictionaryInstaller, FetchError, Fetcher, VerifiedDictionary, VerifiedFile,
//! };
//! use std::fs;
//!
//! struct FixtureFetcher;
//!
//! impl Fetcher for FixtureFetcher {
//!     fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
//!         match url {
//!             "https://example.invalid/example.aff" => Ok(b"SET UTF-8\n".to_vec()),
//!             "https://example.invalid/example.dic" => Ok(b"1\nferrolex\n".to_vec()),
//!             _ => Err(FetchError::Transport("unexpected fixture URL".into())),
//!         }
//!     }
//! }
//!
//! let dictionary = VerifiedDictionary::new(
//!     "example",
//!     "fixture",
//!     "MIT",
//!     "Fixture license",
//!     "https://example.invalid/LICENSE",
//!     VerifiedFile::new(
//!         "example.aff",
//!         "https://example.invalid/example.aff",
//!         "7f6d7c55043d4b09d0a4380720847457b7954048bf1dac70512593006bae8c37",
//!     )?,
//!     VerifiedFile::new(
//!         "example.dic",
//!         "https://example.invalid/example.dic",
//!         "699fd74b184227da79bbb57e50cfe42e362dc08b0206529efdba3f4ffba17f88",
//!     )?,
//! )?;
//! let cache_root = std::env::temp_dir().join("ferrolex-dictionaries-doctest");
//! let installed = DictionaryInstaller::new(FixtureFetcher).install(&dictionary, &cache_root)?;
//! assert!(installed.aff_path().is_file());
//! assert!(installed.dic_path().is_file());
//! fs::remove_dir_all(cache_root)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use fs2::FileExt as _;
use sha2::{Digest as _, Sha256};

/// Immutable `LibreOffice` revision used by the built-in source catalog.
pub const LIBREOFFICE_REVISION: &str = "f2ff99058268502bdcf4cad25c1ca2935ad8aa7d";

const LIBREOFFICE_RAW_BASE: &str = "https://raw.githubusercontent.com/LibreOffice/dictionaries";
const DEFAULT_MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_BODY_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(75);
const STALE_TEMPORARY_FILE_AGE: Duration = Duration::from_secs(60 * 60);
static TEMPORARY_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Text encoding of the upstream Hunspell pair.
///
/// Installation preserves source bytes exactly. The Hunspell import CLI uses
/// this metadata to select a reviewed decoding policy while retaining the
/// source digests for provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceEncoding {
    /// UTF-8 source bytes.
    Utf8,
    /// ISO-8859-1 source bytes.
    Iso8859_1,
    /// ISO-8859-2 source bytes.
    Iso8859_2,
    /// An ASCII-compatible ISO-8859-1 affix file and UTF-8 word list.
    MixedUtf8AndIso8859_1,
    /// A UTF-8-declared affix file with isolated ISO-8859-2 legacy bytes and
    /// a UTF-8 word list.
    MixedUtf8AndIso8859_2Fallback,
}

impl SourceEncoding {
    /// Stable label printed by the command-line catalog.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Iso8859_1 => "ISO-8859-1",
            Self::Iso8859_2 => "ISO-8859-2",
            Self::MixedUtf8AndIso8859_1 => "mixed: AFF ISO-8859-1, DIC UTF-8",
            Self::MixedUtf8AndIso8859_2Fallback => {
                "mixed: AFF UTF-8 with ISO-8859-2 fallback, DIC UTF-8"
            }
        }
    }
}

/// A reviewed `LibreOffice` source declaration with integrity data.
///
/// Every entry contains the SHA-256 digest of its exact upstream bytes at the
/// pinned revision. The catalog is source metadata only; it contains no
/// dictionary content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibreOfficeDictionary {
    locale: &'static str,
    aff_path: &'static str,
    dic_path: &'static str,
    license_notice_path: &'static str,
    license_spdx_expression: &'static str,
    license_label: &'static str,
    encoding: SourceEncoding,
    aff_sha256: &'static str,
    dic_sha256: &'static str,
}

impl LibreOfficeDictionary {
    /// Locale identifier, such as `de_DE`.
    #[must_use]
    pub const fn locale(self) -> &'static str {
        self.locale
    }

    /// Pinned repository revision.
    #[must_use]
    pub const fn revision(self) -> &'static str {
        LIBREOFFICE_REVISION
    }

    /// Locale-specific upstream license or attribution notice path.
    #[must_use]
    pub const fn license_notice_path(self) -> &'static str {
        self.license_notice_path
    }

    /// Reviewed SPDX license identifier or expression for this source pair.
    #[must_use]
    pub const fn license_spdx_expression(self) -> &'static str {
        self.license_spdx_expression
    }

    /// License label recorded by the source catalog.
    #[must_use]
    pub const fn license_label(self) -> &'static str {
        self.license_label
    }

    /// Encoding declared or observed for the exact upstream source pair.
    #[must_use]
    pub const fn encoding(self) -> SourceEncoding {
        self.encoding
    }

    /// Immutable raw URL for the affix file.
    #[must_use]
    pub fn aff_url(self) -> String {
        raw_url(self.aff_path)
    }

    /// Immutable raw URL for the dictionary file.
    #[must_use]
    pub fn dic_url(self) -> String {
        raw_url(self.dic_path)
    }

    /// Immutable raw URL for the locale's license notice.
    #[must_use]
    pub fn license_notice_url(self) -> String {
        raw_url(self.license_notice_path)
    }

    /// Adds reviewed content digests and yields a fetchable manifest.
    ///
    /// `aff_sha256` and `dic_sha256` must be lower or upper case hexadecimal
    /// SHA-256 digests of the bytes at this exact revision. The verifier never
    /// accepts a missing, malformed, or mismatched digest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::InvalidSha256`] if either digest is not exactly
    /// 64 hexadecimal characters.
    pub fn verify(
        self,
        aff_sha256: &str,
        dic_sha256: &str,
    ) -> Result<VerifiedDictionary, ManifestError> {
        Ok(VerifiedDictionary {
            locale: self.locale.to_owned(),
            revision: LIBREOFFICE_REVISION.to_owned(),
            license_spdx_expression: self.license_spdx_expression.to_owned(),
            license_label: self.license_label.to_owned(),
            license_notice_url: self.license_notice_url(),
            aff: VerifiedFile::new(file_name(self.aff_path)?, self.aff_url(), aff_sha256)?,
            dic: VerifiedFile::new(file_name(self.dic_path)?, self.dic_url(), dic_sha256)?,
        })
    }

    /// Returns this catalog entry as its reviewed, fetchable manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] only if a built-in catalog invariant is
    /// violated. Callers receive an error rather than a panic.
    pub fn manifest(self) -> Result<VerifiedDictionary, ManifestError> {
        self.verify(self.aff_sha256, self.dic_sha256)
    }
}

/// The reviewed `LibreOffice` locale sources supported by the installer.
///
/// All entries are pinned to [`LIBREOFFICE_REVISION`] with exact SHA-256
/// digests. Use [`find_locale`] and [`LibreOfficeDictionary::manifest`] before
/// downloading. `CJK` locales are intentionally outside this catalog because
/// text tokenization for them is a separate future capability.
pub const LIBREOFFICE_CATALOG: [LibreOfficeDictionary; 18] = [
    LibreOfficeDictionary {
        locale: "en_US",
        aff_path: "en/en_US.aff",
        dic_path: "en/en_US.dic",
        license_notice_path: "en/license.txt",
        license_spdx_expression: "GPL-2.0-only",
        license_label: "Locale-specific notice in LibreOffice/en/license.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e",
        dic_sha256: "f0b1a234bd178bdd01875b2a392a9647f888b8fe879f79c52aae62c2759b3647",
    },
    LibreOfficeDictionary {
        locale: "de_DE",
        aff_path: "de/de_DE_frami.aff",
        dic_path: "de/de_DE_frami.dic",
        license_notice_path: "de/README_de_DE_frami.txt",
        license_spdx_expression: "GPL-2.0-only OR GPL-3.0-only",
        license_label: "Locale-specific notice in LibreOffice/de/README_de_DE_frami.txt",
        encoding: SourceEncoding::Iso8859_1,
        aff_sha256: "646bf3333ac69c23e9d794533ee5241d6f755c359e8fe10a648f87613743d594",
        dic_sha256: "4ca3c958b0e5545910999bc246f668840bf8ede3df8e5e6790d05edd5a586c38",
    },
    LibreOfficeDictionary {
        locale: "hu_HU",
        aff_path: "hu_HU/hu_HU.aff",
        dic_path: "hu_HU/hu_HU.dic",
        license_notice_path: "hu_HU/README_hu_HU.txt",
        license_spdx_expression: "MPL-2.0-or-later OR LGPL-3.0-or-later",
        license_label: "MPL-2.0-or-later OR LGPL-3.0-or-later (LibreOffice/hu_HU/README_hu_HU.txt)",
        encoding: SourceEncoding::MixedUtf8AndIso8859_2Fallback,
        aff_sha256: "f3a2748dd535cfde2142ab17d0f7f8e4787b03fb25a60829c69ac8d493db4802",
        dic_sha256: "97293d670ad4a3b8e7eebef7e25c6e8e939b914c64b6b4672b2bf416b768f990",
    },
    LibreOfficeDictionary {
        locale: "es_ES",
        aff_path: "es/es_ES.aff",
        dic_path: "es/es_ES.dic",
        license_notice_path: "es/LICENSE.md",
        license_spdx_expression: "GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1",
        license_label: "Locale-specific notice in LibreOffice/es/LICENSE.md",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "e73a9bf8e1383f4986a5dc9e2fbed49371c0c61f511c626d15586bd433c1cad9",
        dic_sha256: "6975dddec3d5d2c676069537bc67b4b5f786c65c5d4cf6703a82acf779ac9ec1",
    },
    LibreOfficeDictionary {
        locale: "fr_FR",
        aff_path: "fr_FR/dictionaries/fr.aff",
        dic_path: "fr_FR/dictionaries/fr.dic",
        license_notice_path: "fr_FR/dictionaries/README_dict_fr.txt",
        license_spdx_expression: "MPL-2.0",
        license_label:
            "Locale-specific notice in LibreOffice/fr_FR/dictionaries/README_dict_fr.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "c176610cd5dc4846806a65ddd029f422d87978bf58f224aa44222662a16a2de5",
        dic_sha256: "b78a868e31dd6e373b6c3217969afb898a9acde828a5e7ef97308da42218c88c",
    },
    LibreOfficeDictionary {
        locale: "it_IT",
        aff_path: "it_IT/it_IT.aff",
        dic_path: "it_IT/it_IT.dic",
        license_notice_path: "it_IT/README_it_IT.txt",
        license_spdx_expression: "GPL-3.0-only",
        license_label: "Locale-specific notice in LibreOffice/it_IT/README_it_IT.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "951afaa19272f13555b8823e8bcf9ccf78f8fe1a07835bdfb912ab3e4d537c2b",
        dic_sha256: "bae1e3501dcd2a923669592493b3fde6c02aae7c7aab83bf5e5b49077e73dd64",
    },
    LibreOfficeDictionary {
        locale: "pt_BR",
        aff_path: "pt_BR/pt_BR.aff",
        dic_path: "pt_BR/pt_BR.dic",
        license_notice_path: "pt_BR/README_pt_BR.txt",
        license_spdx_expression: "LGPL-3.0-only OR MPL-1.1",
        license_label: "Locale-specific notice in LibreOffice/pt_BR/README_pt_BR.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "21d8ad2a769a60e17e2b5ea4ef11d4d593a58b9e2a82d642ef82d6a4c5523865",
        dic_sha256: "a38bfb26b68ece2834e79fe83e48d5792652970ace12db89d1b9674bf9933183",
    },
    LibreOfficeDictionary {
        locale: "pt_PT",
        aff_path: "pt_PT/pt_PT.aff",
        dic_path: "pt_PT/pt_PT.dic",
        license_notice_path: "pt_PT/LICENSES.txt",
        license_spdx_expression: "GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1",
        license_label: "Locale-specific notice in LibreOffice/pt_PT/LICENSES.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "975a209fcc892cb382fa5f34a28c391a39668661ce373ae071287809c5fcae24",
        dic_sha256: "e29ba2d7aa8a2ad43e9cb46ac6473064b661545c87002aea90e18899d98d3cc9",
    },
    LibreOfficeDictionary {
        locale: "nl_NL",
        aff_path: "nl_NL/nl_NL.aff",
        dic_path: "nl_NL/nl_NL.dic",
        license_notice_path: "nl_NL/LICENSE.txt",
        license_spdx_expression: "BSD-3-Clause OR CC-BY-3.0",
        license_label: "Locale-specific notice in LibreOffice/nl_NL/LICENSE.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "f0233d4f721f4661cf5f4d05ed2739549322bf3b6b66764b55a38257e1e16e6f",
        dic_sha256: "bc28af45307700a9927ad5719184da44dfd7eed4f707b8c1477f6d8a21b586a6",
    },
    LibreOfficeDictionary {
        locale: "pl_PL",
        aff_path: "pl_PL/pl_PL.aff",
        dic_path: "pl_PL/pl_PL.dic",
        license_notice_path: "pl_PL/README_pl_PL.txt",
        license_spdx_expression:
            "GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1 OR Apache-2.0 OR CC-BY-4.0",
        license_label: "Locale-specific notice in LibreOffice/pl_PL/README_pl_PL.txt",
        encoding: SourceEncoding::Iso8859_2,
        aff_sha256: "82973651651aa930335c865b339b98db376ca3dbf3a661b70b9eeb71fdf41dca",
        dic_sha256: "c0848440599eb88e5aca500418d5f389e562ec2c157b63dbe39d354658ffba49",
    },
    LibreOfficeDictionary {
        locale: "ru_RU",
        aff_path: "ru_RU/ru_RU.aff",
        dic_path: "ru_RU/ru_RU.dic",
        license_notice_path: "ru_RU/README_ru_RU.txt",
        license_spdx_expression: "BSD-3-Clause",
        license_label: "Locale-specific notice in LibreOffice/ru_RU/README_ru_RU.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "38ce7d4af78e211e9bafe4bf7e3d6a2c420591136cb738ec6648f8fdf6524cd7",
        dic_sha256: "f6047416a0204adbecf3a451b874ec8a97ee37e2cbc714466ef04d8dbcc0d6fc",
    },
    LibreOfficeDictionary {
        locale: "tr_TR",
        aff_path: "tr_TR/tr_TR.aff",
        dic_path: "tr_TR/tr_TR.dic",
        license_notice_path: "tr_TR/LICENSE",
        license_spdx_expression: "MPL-2.0",
        license_label: "Locale-specific notice in LibreOffice/tr_TR/LICENSE",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "a04a227b2ee45574000b876a1afa982e3436d49ebb97a7a796ddc5ec0cc8191b",
        dic_sha256: "48e5352c5770956b24f355265aef7649b25bcb4d933c0b254ea8b419c241f4bd",
    },
    LibreOfficeDictionary {
        locale: "ar",
        aff_path: "ar/ar.aff",
        dic_path: "ar/ar.dic",
        license_notice_path: "ar/COPYING.txt",
        license_spdx_expression: "GPL-2.0-or-later OR LGPL-2.1-or-later OR MPL-1.1",
        license_label: "Locale-specific notice in LibreOffice/ar/COPYING.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "cec30b8621001e49618feb05aec1984c5fcfbf7d2ec309901d5cbf66585217a3",
        dic_sha256: "2a3e5367f61c1583734db9d66734f5603e6be5c2d227cf5c5cd7e4ca586e34fe",
    },
    LibreOfficeDictionary {
        locale: "uk_UA",
        aff_path: "uk_UA/uk_UA.aff",
        dic_path: "uk_UA/uk_UA.dic",
        license_notice_path: "uk_UA/README_uk_UA.txt",
        license_spdx_expression: "MPL-1.1",
        license_label: "Locale-specific notice in LibreOffice/uk_UA/README_uk_UA.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "2219dd15e9802adebc45722c60943b1472640260491af38dd3e43b07e75585e6",
        dic_sha256: "2e5a9e67be63bdb089b3459addb5d71113319d13768e277bcae20f3cc1ad5a93",
    },
    LibreOfficeDictionary {
        locale: "sv_SE",
        aff_path: "sv_SE/dictionaries/sv_SE.aff",
        dic_path: "sv_SE/dictionaries/sv_SE.dic",
        license_notice_path: "sv_SE/LICENSE_sv_SE.txt",
        license_spdx_expression: "LGPL-3.0-only",
        license_label: "Locale-specific notice in LibreOffice/sv_SE/LICENSE_sv_SE.txt",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "b721c9d44bee912feb182b601a1bc2ae3e7dffef660f4130cf2751867488a9dd",
        dic_sha256: "384a2126eff333f5f6f9790ae892554546f53948d2988c600397cb5ad6ce66e8",
    },
    LibreOfficeDictionary {
        locale: "id_ID",
        aff_path: "id/id_ID.aff",
        dic_path: "id/id_ID.dic",
        license_notice_path: "id/LICENSE-dict",
        license_spdx_expression: "LGPL-3.0-only",
        license_label: "Locale-specific notice in LibreOffice/id/LICENSE-dict",
        encoding: SourceEncoding::MixedUtf8AndIso8859_1,
        aff_sha256: "c625d5b237a489c452cf1f9c666600103e8093667dff89e7030a24217995dc79",
        dic_sha256: "775ff17a52801f3d2ee80120952fb44cc9bac6c3cf61740de775e439851a5803",
    },
    LibreOfficeDictionary {
        locale: "hi_IN",
        aff_path: "hi_IN/hi_IN.aff",
        dic_path: "hi_IN/hi_IN.dic",
        license_notice_path: "hi_IN/COPYING",
        license_spdx_expression: "GPL-2.0-only",
        license_label: "Locale-specific notice in LibreOffice/hi_IN/COPYING",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "3ab96772dc3d1cdbec4141798efb8b7a091b92c9acbeb5dfd3c4998a5c508302",
        dic_sha256: "1e01f962a02638ef73e3f8de3c44bfd854f7059d31c9fa96cff0b73a2840f9d9",
    },
    LibreOfficeDictionary {
        locale: "bn_BD",
        aff_path: "bn_BD/bn_BD.aff",
        dic_path: "bn_BD/bn_BD.dic",
        license_notice_path: "bn_BD/COPYING",
        license_spdx_expression: "GPL-2.0-only",
        license_label: "Locale-specific notice in LibreOffice/bn_BD/COPYING",
        encoding: SourceEncoding::Utf8,
        aff_sha256: "6beeacefab0f691cb415c9ab8de227091a3be65510c3d8c0479513b261e61b97",
        dic_sha256: "cfc78b361861a726d22f0654d7c4e0b47f843c4a9e8b605c4c99e91ea683e116",
    },
];

/// Finds one built-in `LibreOffice` source entry by exact locale identifier.
#[must_use]
pub fn find_locale(locale: &str) -> Option<LibreOfficeDictionary> {
    LIBREOFFICE_CATALOG
        .iter()
        .copied()
        .find(|dictionary| dictionary.locale == locale)
}

/// A digest-pinned dictionary manifest that may be downloaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedDictionary {
    locale: String,
    revision: String,
    license_spdx_expression: String,
    license_label: String,
    license_notice_url: String,
    aff: VerifiedFile,
    dic: VerifiedFile,
}

impl VerifiedDictionary {
    /// Creates a custom reviewed manifest, useful for a source registry.
    ///
    /// # Errors
    ///
    /// Returns an error when the locale would escape a cache directory, the
    /// SPDX expression is empty, or the license notice is not an absolute HTTPS
    /// URL.
    pub fn new(
        locale: impl Into<String>,
        revision: impl Into<String>,
        license_spdx_expression: impl Into<String>,
        license_label: impl Into<String>,
        license_notice_url: impl Into<String>,
        aff: VerifiedFile,
        dic: VerifiedFile,
    ) -> Result<Self, ManifestError> {
        let locale = locale.into();
        if !is_safe_locale(&locale) {
            return Err(ManifestError::UnsafeLocale(locale));
        }
        let license_spdx_expression = license_spdx_expression.into();
        if license_spdx_expression.trim().is_empty() {
            return Err(ManifestError::MissingLicenseExpression);
        }
        let license_notice_url = license_notice_url.into();
        if !is_https_url(&license_notice_url) {
            return Err(ManifestError::InsecureUrl);
        }
        Ok(Self {
            locale,
            revision: revision.into(),
            license_spdx_expression,
            license_label: license_label.into(),
            license_notice_url,
            aff,
            dic,
        })
    }

    /// Locale identifying the dedicated cache directory.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Upstream revision recorded by the review manifest.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Reviewed SPDX license identifier or expression for the source pair.
    #[must_use]
    pub fn license_spdx_expression(&self) -> &str {
        &self.license_spdx_expression
    }

    /// Source's locale-specific license label.
    #[must_use]
    pub fn license_label(&self) -> &str {
        &self.license_label
    }

    /// HTTPS URL for the source's locale-specific license notice.
    #[must_use]
    pub fn license_notice_url(&self) -> &str {
        &self.license_notice_url
    }

    /// Reviewed affix file descriptor.
    #[must_use]
    pub fn aff(&self) -> &VerifiedFile {
        &self.aff
    }

    /// Reviewed word-list file descriptor.
    #[must_use]
    pub fn dic(&self) -> &VerifiedFile {
        &self.dic
    }
}

/// One remotely acquired file with a mandatory SHA-256 digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedFile {
    name: String,
    url: String,
    sha256: [u8; 32],
}

impl VerifiedFile {
    /// Creates one safe HTTPS file descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache file name can escape its locale directory,
    /// the URL is not absolute HTTPS, or the digest is malformed.
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        sha256: &str,
    ) -> Result<Self, ManifestError> {
        let name = name.into();
        if !is_safe_file_name(&name) {
            return Err(ManifestError::UnsafeFileName(name));
        }
        let url = url.into();
        if !is_https_url(&url) {
            return Err(ManifestError::InsecureUrl);
        }
        Ok(Self {
            name,
            url,
            sha256: parse_sha256(sha256)?,
        })
    }

    /// Cache file name, which contains no path separators.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// HTTPS source URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Expected SHA-256 in lowercase hexadecimal.
    #[must_use]
    pub fn sha256_hex(&self) -> String {
        hex_digest(&self.sha256)
    }
}

/// Transport abstraction so verification can be tested without a network.
pub trait Fetcher {
    /// Gets all response bytes for one HTTPS URL.
    ///
    /// # Errors
    ///
    /// Implementations return [`FetchError`] when the transport or response
    /// cannot satisfy their acquisition policy.
    fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError>;

    /// Gets response bytes while enforcing a caller-selected upper bound.
    ///
    /// The default implementation preserves compatibility with fetchers that
    /// acquire the complete response before checking its length. Streaming
    /// transports should override this method so they can stop reading after
    /// `maximum_file_bytes + 1` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::FileTooLarge`] when the response exceeds
    /// `maximum_file_bytes`, or another [`FetchError`] from [`Self::fetch`].
    fn fetch_with_limit(
        &self,
        url: &str,
        maximum_file_bytes: usize,
    ) -> Result<Vec<u8>, FetchError> {
        enforce_response_limit(url, self.fetch(url)?, maximum_file_bytes)
    }
}

/// HTTPS fetcher backed by `ureq` and rustls root certificates.
#[derive(Clone, Copy, Debug, Default)]
pub struct UreqFetcher;

impl UreqFetcher {
    fn agent() -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .max_redirects(0)
                .timeout_connect(Some(CONNECT_TIMEOUT))
                .timeout_recv_response(Some(RESPONSE_HEADER_TIMEOUT))
                .timeout_recv_body(Some(RESPONSE_BODY_TIMEOUT))
                .timeout_global(Some(REQUEST_TIMEOUT))
                .build(),
        )
    }
}

impl Fetcher for UreqFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.fetch_with_limit(url, DEFAULT_MAX_FILE_BYTES)
    }

    fn fetch_with_limit(
        &self,
        url: &str,
        maximum_file_bytes: usize,
    ) -> Result<Vec<u8>, FetchError> {
        if !is_https_url(url) {
            return Err(FetchError::InsecureUrl(url.to_owned()));
        }
        let response = Self::agent()
            .get(url)
            .call()
            .map_err(|error| map_ureq_error(url, error))?;
        reject_redirect(url, response.status())?;
        let mut reader = response.into_body().into_reader();
        read_response_with_limit(url, &mut reader, maximum_file_bytes)
    }
}

fn read_response_with_limit(
    url: &str,
    reader: &mut impl Read,
    maximum_file_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let read_limit = u64::try_from(maximum_file_bytes.saturating_add(1)).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    reader
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| map_response_read_error(url, source))?;
    enforce_response_limit(url, bytes, maximum_file_bytes)
}

fn enforce_response_limit(
    url: &str,
    bytes: Vec<u8>,
    maximum_file_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    if bytes.len() > maximum_file_bytes {
        return Err(FetchError::FileTooLarge {
            url: url.to_owned(),
            limit: maximum_file_bytes,
            actual: bytes.len(),
        });
    }
    Ok(bytes)
}

/// Installer which owns no cache location; callers provide it per operation.
#[derive(Clone, Copy, Debug)]
pub struct DictionaryInstaller<F> {
    fetcher: F,
    maximum_file_bytes: usize,
}

impl<F> DictionaryInstaller<F> {
    /// Creates an installer with a 64 MiB per-file upper bound.
    #[must_use]
    pub const fn new(fetcher: F) -> Self {
        Self {
            fetcher,
            maximum_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }

    /// Sets a caller-reviewed per-file response limit.
    #[must_use]
    pub const fn with_maximum_file_bytes(mut self, maximum_file_bytes: usize) -> Self {
        self.maximum_file_bytes = maximum_file_bytes;
        self
    }
}

impl<F: Fetcher> DictionaryInstaller<F> {
    /// Downloads, verifies, and atomically installs both dictionary files.
    ///
    /// The cache root is never inferred from environment variables. The files
    /// land at `<cache_root>/<locale>/<manifest-file-name>`.
    ///
    /// # Errors
    ///
    /// Returns [`FetchError`] if transport, size, digest verification, or the
    /// atomic cache write fails. Existing cache files with mismatched bytes are
    /// left untouched and return [`FetchError::CacheConflict`].
    pub fn install(
        &self,
        dictionary: &VerifiedDictionary,
        cache_root: &Path,
    ) -> Result<InstalledDictionary, FetchError> {
        let directory = cache_root.join(dictionary.locale());
        fs::create_dir_all(&directory).map_err(|source| FetchError::CreateCache {
            path: directory.clone(),
            source,
        })?;
        let aff_path = self.install_file(dictionary.aff(), &directory)?;
        let dic_path = self.install_file(dictionary.dic(), &directory)?;
        Ok(InstalledDictionary { aff_path, dic_path })
    }

    fn install_file(&self, file: &VerifiedFile, directory: &Path) -> Result<PathBuf, FetchError> {
        let destination = directory.join(file.name());
        if destination.exists() {
            if cache_matches(&destination, file.sha256)? {
                return Ok(destination);
            }
            return Err(FetchError::CacheConflict(destination));
        }

        let bytes = self
            .fetcher
            .fetch_with_limit(file.url(), self.maximum_file_bytes)?;
        if bytes.len() > self.maximum_file_bytes {
            return Err(FetchError::FileTooLarge {
                url: file.url().to_owned(),
                limit: self.maximum_file_bytes,
                actual: bytes.len(),
            });
        }
        let actual = sha256(&bytes);
        if actual != file.sha256 {
            return Err(FetchError::ChecksumMismatch {
                url: file.url().to_owned(),
                expected: file.sha256_hex(),
                actual: hex_digest(&actual),
            });
        }

        atomic_write_new(&destination, &bytes, file.sha256)?;
        Ok(destination)
    }
}

/// Result paths after a successful cache installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledDictionary {
    aff_path: PathBuf,
    dic_path: PathBuf,
}

impl InstalledDictionary {
    /// Cached affix path.
    #[must_use]
    pub fn aff_path(&self) -> &Path {
        &self.aff_path
    }

    /// Cached dictionary path.
    #[must_use]
    pub fn dic_path(&self) -> &Path {
        &self.dic_path
    }
}

/// A rejected source or digest declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManifestError {
    /// Cache component was not a safe locale identifier.
    UnsafeLocale(String),
    /// Cache file name would escape its assigned locale directory.
    UnsafeFileName(String),
    /// Source URL was not absolute HTTPS.
    InsecureUrl,
    /// SPDX license identity was missing from the manifest.
    MissingLicenseExpression,
    /// SHA-256 value was malformed.
    InvalidSha256(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeLocale(locale) => write!(formatter, "unsafe locale `{locale}`"),
            Self::UnsafeFileName(name) => write!(formatter, "unsafe cache file name `{name}`"),
            Self::InsecureUrl => formatter.write_str("dictionary sources must use HTTPS"),
            Self::MissingLicenseExpression => {
                formatter.write_str("dictionary manifest requires an SPDX license expression")
            }
            Self::InvalidSha256(value) => write!(formatter, "invalid SHA-256 digest `{value}`"),
        }
    }
}

impl Error for ManifestError {}

/// An acquisition, integrity, or cache failure.
#[derive(Debug)]
pub enum FetchError {
    /// The requested URL is not HTTPS.
    InsecureUrl(String),
    /// The HTTP client rejected or could not complete the request.
    Transport(String),
    /// The source returned an HTTP redirect that was not followed.
    Redirect {
        /// Original source URL that returned the redirect.
        url: String,
        /// HTTP redirect status code.
        status: u16,
    },
    /// The HTTP client exceeded a configured timeout while fetching a source.
    Timeout {
        /// Source URL whose transfer did not complete in time.
        url: String,
        /// Transfer stage at which the timeout occurred.
        stage: String,
    },
    /// The response body could not be read.
    Read(io::Error),
    /// Response exceeded the caller's per-file bound.
    FileTooLarge {
        /// Source URL whose response exceeded the configured bound.
        url: String,
        /// Configured maximum response size in bytes.
        limit: usize,
        /// Observed response size in bytes.
        actual: usize,
    },
    /// Bytes differed from the reviewed manifest.
    ChecksumMismatch {
        /// Source URL whose bytes did not match the reviewed digest.
        url: String,
        /// Reviewed SHA-256 digest.
        expected: String,
        /// SHA-256 digest calculated from the received bytes.
        actual: String,
    },
    /// Cache root or locale directory could not be created.
    CreateCache {
        /// Cache path that could not be created.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// Existing cache data could not be read for conflict detection.
    ReadCache {
        /// Existing cache path that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// A cache file already occupied the target path and was left untouched.
    CacheConflict(PathBuf),
    /// Temporary cache file could not be created, written, or atomically moved.
    WriteCache {
        /// Target cache path that could not be written or atomically moved.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
}

impl fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsecureUrl(url) => write!(formatter, "refusing non-HTTPS URL `{url}`"),
            Self::Transport(message) => write!(formatter, "dictionary download failed: {message}"),
            Self::Redirect { url, status } => {
                write!(formatter, "refusing HTTP redirect {status} from `{url}`")
            }
            Self::Timeout { url, stage } => {
                write!(
                    formatter,
                    "dictionary download timed out while {stage} for `{url}`"
                )
            }
            Self::Read(source) => write!(formatter, "could not read dictionary response: {source}"),
            Self::FileTooLarge { url, limit, actual } => {
                write!(
                    formatter,
                    "dictionary response `{url}` is {actual} bytes, above {limit} byte limit"
                )
            }
            Self::ChecksumMismatch {
                url,
                expected,
                actual,
            } => write!(
                formatter,
                "SHA-256 mismatch for `{url}` (expected {expected}, got {actual})"
            ),
            Self::CreateCache { path, source } => {
                write!(
                    formatter,
                    "could not create cache directory `{}`: {source}",
                    path.display()
                )
            }
            Self::ReadCache { path, source } => {
                write!(
                    formatter,
                    "could not read cache file `{}`: {source}",
                    path.display()
                )
            }
            Self::CacheConflict(path) => write!(
                formatter,
                "cache file `{}` is already occupied; its bytes were not replaced",
                path.display()
            ),
            Self::WriteCache { path, source } => {
                write!(
                    formatter,
                    "could not atomically write cache file `{}`: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for FetchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(source)
            | Self::CreateCache { source, .. }
            | Self::ReadCache { source, .. }
            | Self::WriteCache { source, .. } => Some(source),
            Self::InsecureUrl(_)
            | Self::Transport(_)
            | Self::Redirect { .. }
            | Self::Timeout { .. }
            | Self::FileTooLarge { .. }
            | Self::ChecksumMismatch { .. }
            | Self::CacheConflict(_) => None,
        }
    }
}

fn raw_url(path: &str) -> String {
    format!("{LIBREOFFICE_RAW_BASE}/{LIBREOFFICE_REVISION}/{path}")
}

fn file_name(path: &str) -> Result<&str, ManifestError> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_safe_file_name(name))
        .ok_or_else(|| ManifestError::UnsafeFileName(path.to_owned()))
}

fn is_safe_locale(locale: &str) -> bool {
    !locale.is_empty()
        && locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn is_safe_file_name(name: &str) -> bool {
    Path::new(name)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
        && Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name)
}

fn is_https_url(url: &str) -> bool {
    url.starts_with("https://") && !url[8..].is_empty() && !url[8..].starts_with('/')
}

fn parse_sha256(value: &str) -> Result<[u8; 32], ManifestError> {
    if value.len() != 64 {
        return Err(ManifestError::InvalidSha256(value.to_owned()));
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| ManifestError::InvalidSha256(value.to_owned()))?;
    }
    Ok(digest)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in digest {
        write!(&mut value, "{byte:02x}").expect("writing to String does not fail");
    }
    value
}

fn map_ureq_error(url: &str, error: ureq::Error) -> FetchError {
    match error {
        ureq::Error::Timeout(timeout) => timeout_error(url, timeout),
        other => FetchError::Transport(other.to_string()),
    }
}

fn reject_redirect(url: &str, status: ureq::http::StatusCode) -> Result<(), FetchError> {
    if status.is_redirection() {
        return Err(FetchError::Redirect {
            url: url.to_owned(),
            status: status.as_u16(),
        });
    }
    Ok(())
}

fn map_response_read_error(url: &str, source: io::Error) -> FetchError {
    let timeout = source
        .get_ref()
        .and_then(|error| error.downcast_ref::<ureq::Error>())
        .and_then(|error| match error {
            ureq::Error::Timeout(timeout) => Some(*timeout),
            _ => None,
        });
    match timeout {
        Some(timeout) => timeout_error(url, timeout),
        None => FetchError::Read(source),
    }
}

fn timeout_error(url: &str, timeout: ureq::Timeout) -> FetchError {
    FetchError::Timeout {
        url: url.to_owned(),
        stage: timeout.to_string(),
    }
}

fn cache_matches(destination: &Path, expected_sha256: [u8; 32]) -> Result<bool, FetchError> {
    let existing = fs::read(destination).map_err(|source| FetchError::ReadCache {
        path: destination.to_path_buf(),
        source,
    })?;
    Ok(sha256(&existing) == expected_sha256)
}

fn atomic_write_new(
    destination: &Path,
    bytes: &[u8],
    expected_sha256: [u8; 32],
) -> Result<(), FetchError> {
    atomic_write_new_with_hard_link(destination, bytes, expected_sha256, |source, target| {
        fs::hard_link(source, target)
    })
}

fn atomic_write_new_with_hard_link(
    destination: &Path,
    bytes: &[u8],
    expected_sha256: [u8; 32],
    hard_link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), FetchError> {
    sweep_stale_temporary_siblings(destination);
    let temporary = temporary_sibling(destination);
    let parent = destination.parent().unwrap_or(Path::new("."));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| FetchError::WriteCache {
                path: temporary.clone(),
                source,
            })?;
        created = true;
        file.write_all(bytes)
            .map_err(|source| FetchError::WriteCache {
                path: temporary.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| FetchError::WriteCache {
            path: temporary.clone(),
            source,
        })?;
        match hard_link(&temporary, destination) {
            Ok(()) => {
                fs::remove_file(&temporary).map_err(|source| FetchError::WriteCache {
                    path: temporary.clone(),
                    source,
                })?;
                sync_parent_directory(parent).map_err(|source| FetchError::WriteCache {
                    path: parent.to_path_buf(),
                    source,
                })
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                if cache_matches(destination, expected_sha256)? {
                    fs::remove_file(&temporary).map_err(|source| FetchError::WriteCache {
                        path: temporary.clone(),
                        source,
                    })?;
                    sync_parent_directory(parent).map_err(|source| FetchError::WriteCache {
                        path: parent.to_path_buf(),
                        source,
                    })
                } else {
                    Err(FetchError::CacheConflict(destination.to_path_buf()))
                }
            }
            Err(_) => rename_without_hard_links(&temporary, destination, expected_sha256, parent),
        }
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn rename_without_hard_links(
    temporary: &Path,
    destination: &Path,
    expected_sha256: [u8; 32],
    parent: &Path,
) -> Result<(), FetchError> {
    let _lock = InstallLock::acquire(destination)?;
    if destination.exists() {
        if cache_matches(destination, expected_sha256)? {
            fs::remove_file(temporary).map_err(|source| FetchError::WriteCache {
                path: temporary.to_path_buf(),
                source,
            })?;
            return sync_parent_directory(parent).map_err(|source| FetchError::WriteCache {
                path: parent.to_path_buf(),
                source,
            });
        }
        return Err(FetchError::CacheConflict(destination.to_path_buf()));
    }
    fs::rename(temporary, destination).map_err(|source| FetchError::WriteCache {
        path: destination.to_path_buf(),
        source,
    })?;
    if !cache_matches(destination, expected_sha256)? {
        return Err(FetchError::CacheConflict(destination.to_path_buf()));
    }
    sync_parent_directory(parent).map_err(|source| FetchError::WriteCache {
        path: parent.to_path_buf(),
        source,
    })
}

struct InstallLock {
    file: fs::File,
}

impl InstallLock {
    fn acquire(destination: &Path) -> Result<Self, FetchError> {
        let path = hidden_sibling(destination, "install-lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| FetchError::WriteCache {
                path: path.clone(),
                source,
            })?;
        file.lock_exclusive()
            .map_err(|source| FetchError::WriteCache { path, source })?;
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn hidden_sibling(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or(std::ffi::OsStr::new("dictionary")),
    );
    name.push(".");
    name.push(suffix);
    parent.join(name)
}

fn temporary_sibling(destination: &Path) -> PathBuf {
    hidden_sibling(
        destination,
        &format!(
            "tmp-{}-{}",
            std::process::id(),
            TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
    )
}

fn sweep_stale_temporary_siblings(destination: &Path) {
    let parent = destination.parent().unwrap_or(Path::new("."));
    let Some(name) = destination.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let current_prefix = format!(".{name}.tmp-");
    let legacy_prefix = destination
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| format!("{stem}.tmp-"));
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let belongs_to_destination = file_name.starts_with(&current_prefix)
            || legacy_prefix
                .as_deref()
                .is_some_and(|prefix| file_name.starts_with(prefix));
        if belongs_to_destination && file_is_stale(&entry.path(), STALE_TEMPORARY_FILE_AGE) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn file_is_stale(path: &Path, maximum_age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= maximum_age)
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "all platforms share one fallible directory-sync call site"
)]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{self, Cursor};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    use super::{
        atomic_write_new_with_hard_link, enforce_response_limit, find_locale,
        map_response_read_error, map_ureq_error, read_response_with_limit, reject_redirect, sha256,
        DictionaryInstaller, FetchError, Fetcher, LibreOfficeDictionary, ManifestError,
        SourceEncoding, UreqFetcher, VerifiedDictionary, VerifiedFile, CONNECT_TIMEOUT,
        DEFAULT_MAX_FILE_BYTES, LIBREOFFICE_CATALOG, LIBREOFFICE_REVISION, REQUEST_TIMEOUT,
        RESPONSE_BODY_TIMEOUT, RESPONSE_HEADER_TIMEOUT, STALE_TEMPORARY_FILE_AGE,
    };

    const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    static NEXT_CACHE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn catalog_pins_all_requested_locales_and_exact_upstream_paths() {
        assert_eq!(LIBREOFFICE_CATALOG.len(), 18);
        for locale in [
            "en_US", "de_DE", "hu_HU", "es_ES", "fr_FR", "it_IT", "pt_BR", "pt_PT", "nl_NL",
            "pl_PL", "ru_RU", "tr_TR", "ar", "uk_UA", "sv_SE", "id_ID", "hi_IN", "bn_BD",
        ] {
            let dictionary = find_locale(locale).expect("catalog contains requested locale");
            assert_eq!(dictionary.revision(), LIBREOFFICE_REVISION);
            assert!(dictionary.aff_url().starts_with("https://"));
            assert!(dictionary.dic_url().starts_with("https://"));
            assert!(dictionary.license_notice_url().starts_with("https://"));
            assert!(!dictionary.license_spdx_expression().is_empty());
            assert!(!dictionary.license_label().is_empty());
            let manifest = dictionary.manifest().expect("catalog digest is valid");
            assert_eq!(manifest.locale(), locale);
            assert_eq!(manifest.revision(), LIBREOFFICE_REVISION);
            assert_eq!(
                manifest.license_spdx_expression(),
                dictionary.license_spdx_expression()
            );
            assert_eq!(manifest.aff().sha256_hex().len(), 64);
            assert_eq!(manifest.dic().sha256_hex().len(), 64);
        }
        assert!(find_locale("en_GB").is_none());
        assert!(find_locale("de_DE")
            .expect("German exists")
            .aff_url()
            .ends_with("/de/de_DE_frami.aff"));
        assert!(find_locale("fr_FR")
            .expect("French exists")
            .dic_url()
            .ends_with("/fr_FR/dictionaries/fr.dic"));
        assert_eq!(
            find_locale("pl_PL").expect("Polish exists").encoding(),
            SourceEncoding::Iso8859_2
        );
        assert_eq!(
            find_locale("hu_HU").expect("Hungarian exists").encoding(),
            SourceEncoding::MixedUtf8AndIso8859_2Fallback
        );
    }

    #[test]
    fn catalog_exposes_reviewed_spdx_expressions_for_each_locale() {
        for (locale, expected) in [
            ("en_US", "GPL-2.0-only"),
            ("de_DE", "GPL-2.0-only OR GPL-3.0-only"),
            ("hu_HU", "MPL-2.0-or-later OR LGPL-3.0-or-later"),
            ("es_ES", "GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1"),
            ("fr_FR", "MPL-2.0"),
            ("it_IT", "GPL-3.0-only"),
            ("pt_BR", "LGPL-3.0-only OR MPL-1.1"),
            ("pt_PT", "GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1"),
            ("nl_NL", "BSD-3-Clause OR CC-BY-3.0"),
            (
                "pl_PL",
                "GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1 OR Apache-2.0 OR CC-BY-4.0",
            ),
            ("ru_RU", "BSD-3-Clause"),
            ("tr_TR", "MPL-2.0"),
            ("ar", "GPL-2.0-or-later OR LGPL-2.1-or-later OR MPL-1.1"),
            ("uk_UA", "MPL-1.1"),
            ("sv_SE", "LGPL-3.0-only"),
            ("id_ID", "LGPL-3.0-only"),
            ("hi_IN", "GPL-2.0-only"),
            ("bn_BD", "GPL-2.0-only"),
        ] {
            assert_eq!(
                find_locale(locale)
                    .expect("catalogued locale")
                    .license_spdx_expression(),
                expected
            );
        }
    }

    #[test]
    fn source_catalog_exposes_reviewed_digests_without_caller_input() {
        let source = find_locale("en_US").expect("English exists");
        let manifest = source
            .manifest()
            .expect("catalog creates a verified manifest");
        assert_eq!(manifest.locale(), "en_US");
        assert_eq!(manifest.revision(), LIBREOFFICE_REVISION);
        assert_eq!(
            manifest.aff().sha256_hex(),
            "e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e"
        );
        assert!(matches!(
            source.verify("not-a-digest", SHA256_ABC),
            Err(ManifestError::InvalidSha256(_))
        ));
    }

    #[test]
    fn installer_verifies_then_atomically_writes_only_checked_bytes() {
        let manifest = manifest();
        let fetcher = FixtureFetcher::from([
            (manifest.aff().url().to_owned(), Ok(b"abc".to_vec())),
            (manifest.dic().url().to_owned(), Ok(b"abc".to_vec())),
        ]);
        let cache = Cache::new();

        let installed = DictionaryInstaller::new(fetcher)
            .install(&manifest, cache.path())
            .expect("checked fixture bytes install");

        assert_eq!(fs::read(installed.aff_path()).expect("aff exists"), b"abc");
        assert_eq!(fs::read(installed.dic_path()).expect("dic exists"), b"abc");
        assert_eq!(installed.aff_path(), cache.path().join("en_US/sample.aff"));
    }

    #[test]
    fn unsupported_hard_links_fall_back_to_verified_rename_and_sweep_stale_temps() {
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        fs::create_dir_all(&directory).expect("cache directory is writable");
        let destination = directory.join("sample.aff");
        let stale_temporary = directory.join("sample.tmp-stale");
        let fresh_temporary = directory.join(".sample.aff.tmp-active");
        fs::write(&stale_temporary, b"stale").expect("stale fixture is writable");
        fs::write(&fresh_temporary, b"active").expect("active fixture is writable");
        let stale_time = SystemTime::now()
            .checked_sub(STALE_TEMPORARY_FILE_AGE + Duration::from_secs(1))
            .expect("fixture timestamp remains representable");
        fs::File::options()
            .write(true)
            .open(&stale_temporary)
            .expect("stale fixture opens")
            .set_times(fs::FileTimes::new().set_modified(stale_time))
            .expect("stale fixture timestamp is writable");
        let bytes = b"abc";

        atomic_write_new_with_hard_link(&destination, bytes, sha256(bytes), |_, _| {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "fixture filesystem has no hard links",
            ))
        })
        .expect("verified rename fallback installs the cache entry");

        assert_eq!(
            fs::read(destination).expect("fallback output is readable"),
            bytes
        );
        assert!(!stale_temporary.exists());
        assert!(fresh_temporary.exists());
        assert!(directory.join(".sample.aff.install-lock").exists());
    }

    #[test]
    fn rename_fallback_preserves_a_conflicting_cache_entry() {
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        fs::create_dir_all(&directory).expect("cache directory is writable");
        let destination = directory.join("sample.aff");
        fs::write(&destination, b"different").expect("conflicting fixture is writable");
        let bytes = b"abc";

        let error = atomic_write_new_with_hard_link(&destination, bytes, sha256(bytes), |_, _| {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "fixture filesystem has no hard links",
            ))
        })
        .expect_err("rename fallback refuses a conflicting cache entry");

        assert!(matches!(error, FetchError::CacheConflict(path) if path == destination));
        assert_eq!(
            fs::read(&destination).expect("conflicting bytes remain readable"),
            b"different"
        );
        assert!(directory.join(".sample.aff.install-lock").exists());
    }

    #[test]
    fn failed_digest_never_creates_a_target_file() {
        let manifest = manifest();
        let fetcher = FixtureFetcher::from([(
            manifest.aff().url().to_owned(),
            Ok(b"not-the-expected-bytes".to_vec()),
        )]);
        let cache = Cache::new();

        let error = DictionaryInstaller::new(fetcher)
            .install(&manifest, cache.path())
            .expect_err("mismatched bytes are rejected");

        assert!(matches!(error, FetchError::ChecksumMismatch { .. }));
        assert!(!cache.path().join("en_US/sample.aff").exists());
    }

    #[test]
    fn cache_root_is_caller_controlled_and_conflicts_are_not_overwritten() {
        let manifest = manifest();
        let fetcher =
            FixtureFetcher::from([(manifest.aff().url().to_owned(), Ok(b"abc".to_vec()))]);
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        fs::create_dir_all(&directory).expect("cache directory is writable");
        fs::write(directory.join("sample.aff"), b"different").expect("conflict can be prepared");

        let error = DictionaryInstaller::new(fetcher)
            .install(&manifest, cache.path())
            .expect_err("existing different bytes are preserved");

        assert!(matches!(error, FetchError::CacheConflict(_)));
        assert_eq!(
            fs::read(directory.join("sample.aff")).expect("conflict remains"),
            b"different"
        );
    }

    #[test]
    fn concurrent_cache_creation_returns_conflict_without_replacing_bytes() {
        let manifest = manifest();
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        let destination = directory.join("sample.aff");

        let error = DictionaryInstaller::new(RacingFetcher {
            destination: destination.clone(),
            competing_bytes: b"different".to_vec(),
        })
        .install(&manifest, cache.path())
        .expect_err("a file created after the cache check is not overwritten");

        assert!(matches!(error, FetchError::CacheConflict(path) if path == destination));
        assert_eq!(
            fs::read(destination).expect("concurrent cache bytes remain"),
            b"different"
        );
    }

    #[test]
    fn concurrent_cache_creation_accepts_identical_verified_bytes() {
        let manifest = manifest();
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        let destination = directory.join("sample.aff");

        let installed = DictionaryInstaller::new(RacingFetcher {
            destination: destination.clone(),
            competing_bytes: b"abc".to_vec(),
        })
        .install(&manifest, cache.path())
        .expect("an identical concurrent cache entry is reusable");

        assert_eq!(installed.aff_path(), destination);
        assert_eq!(
            fs::read(installed.aff_path()).expect("verified concurrent bytes remain"),
            b"abc"
        );
        assert_eq!(
            fs::read(installed.dic_path()).expect("second cache file is installed"),
            b"abc"
        );
        let mut cached_names = fs::read_dir(directory)
            .expect("cache directory is readable")
            .map(|entry| {
                entry
                    .expect("cache entry is readable")
                    .file_name()
                    .into_string()
                    .expect("fixture names are UTF-8")
            })
            .collect::<Vec<_>>();
        cached_names.sort();
        assert_eq!(cached_names, ["sample.aff", "sample.dic"]);
    }

    #[test]
    fn ureq_fetcher_uses_documented_timeouts_and_reports_the_timeout_stage() {
        let timeouts = UreqFetcher::agent().config().timeouts();
        assert_eq!(timeouts.connect, Some(CONNECT_TIMEOUT));
        assert_eq!(timeouts.recv_response, Some(RESPONSE_HEADER_TIMEOUT));
        assert_eq!(timeouts.recv_body, Some(RESPONSE_BODY_TIMEOUT));
        assert_eq!(timeouts.global, Some(REQUEST_TIMEOUT));
        assert!(CONNECT_TIMEOUT < RESPONSE_BODY_TIMEOUT);
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(75));

        let url = "https://example.test/source.dic";
        let connect_error = map_ureq_error(url, ureq::Error::Timeout(ureq::Timeout::Connect));
        assert!(matches!(
            connect_error,
            FetchError::Timeout { url: timeout_url, stage }
                if timeout_url == url && stage == "connect"
        ));

        let read_error =
            map_response_read_error(url, ureq::Error::Timeout(ureq::Timeout::RecvBody).into_io());
        assert!(matches!(
            read_error,
            FetchError::Timeout { url: timeout_url, stage }
                if timeout_url == url && stage == "receive body"
        ));
    }

    #[test]
    fn redirect_statuses_are_refused_with_the_original_url() {
        let url = "https://example.test/source.dic";
        let error = reject_redirect(url, ureq::http::StatusCode::TEMPORARY_REDIRECT)
            .expect_err("redirects are rejected before their response body is read");
        let message = error.to_string();

        assert!(matches!(
            error,
            FetchError::Redirect {
                url: redirect_url,
                status: 307
            } if redirect_url == url
        ));
        assert_eq!(
            message,
            "refusing HTTP redirect 307 from `https://example.test/source.dic`"
        );
        reject_redirect(url, ureq::http::StatusCode::OK)
            .expect("successful statuses continue to body processing");
    }

    #[test]
    fn verified_cache_entries_are_reused_without_a_fetch() {
        let manifest = manifest();
        let cache = Cache::new();
        let directory = cache.path().join("en_US");
        fs::create_dir_all(&directory).expect("cache directory is writable");
        fs::write(directory.join("sample.aff"), b"abc").expect("aff cache can be prepared");
        fs::write(directory.join("sample.dic"), b"abc").expect("dic cache can be prepared");

        let installed = DictionaryInstaller::new(FixtureFetcher::from([]))
            .install(&manifest, cache.path())
            .expect("verified cache needs no network response");

        assert_eq!(installed.aff_path(), directory.join("sample.aff"));
        assert_eq!(installed.dic_path(), directory.join("sample.dic"));
    }

    #[test]
    fn manifest_rejects_path_escapes_and_non_https_sources() {
        assert!(matches!(
            VerifiedFile::new("../escape", "https://example.test/file", SHA256_ABC),
            Err(ManifestError::UnsafeFileName(_))
        ));
        assert!(matches!(
            VerifiedFile::new("file.aff", "http://example.test/file", SHA256_ABC),
            Err(ManifestError::InsecureUrl)
        ));
        let file = VerifiedFile::new("file.aff", "https://example.test/file", SHA256_ABC)
            .expect("file is valid");
        assert!(matches!(
            VerifiedDictionary::new(
                "../en_US",
                "revision",
                "GPL-2.0-only",
                "notice",
                "https://example.test/license",
                file.clone(),
                file.clone(),
            ),
            Err(ManifestError::UnsafeLocale(_))
        ));
        assert!(matches!(
            VerifiedDictionary::new(
                "en_US",
                "revision",
                " ",
                "notice",
                "https://example.test/license",
                file.clone(),
                file,
            ),
            Err(ManifestError::MissingLicenseExpression)
        ));
    }

    #[test]
    fn caller_file_limit_is_checked_before_writing() {
        let manifest = manifest();
        let fetcher =
            FixtureFetcher::from([(manifest.aff().url().to_owned(), Ok(b"abc".to_vec()))]);
        let cache = Cache::new();

        let error = DictionaryInstaller::new(fetcher)
            .with_maximum_file_bytes(2)
            .install(&manifest, cache.path())
            .expect_err("response above policy is rejected");

        assert!(matches!(error, FetchError::FileTooLarge { limit: 2, .. }));
        assert!(!cache.path().join("en_US/sample.aff").exists());
    }

    #[test]
    fn installer_threads_raised_and_lowered_limits_into_the_fetcher() {
        let manifest = manifest();
        let fetcher = LimitRecordingFetcher {
            requested_limit: Cell::new(None),
        };
        let raised_limit = DEFAULT_MAX_FILE_BYTES * 4;
        let installer = DictionaryInstaller::new(fetcher).with_maximum_file_bytes(raised_limit);
        let cache = Cache::new();

        installer
            .install(&manifest, cache.path())
            .expect("the raised caller limit reaches both fixture fetches");
        assert_eq!(installer.fetcher.requested_limit.get(), Some(raised_limit));

        let fetcher = LimitRecordingFetcher {
            requested_limit: Cell::new(None),
        };
        let installer = DictionaryInstaller::new(fetcher).with_maximum_file_bytes(2);
        let cache = Cache::new();
        let error = installer
            .install(&manifest, cache.path())
            .expect_err("the lowered caller limit rejects the fixture response");
        assert!(matches!(
            error,
            FetchError::FileTooLarge {
                limit: 2,
                actual: 3,
                ..
            }
        ));
        assert_eq!(installer.fetcher.requested_limit.get(), Some(2));
    }

    #[test]
    fn streaming_response_reader_stops_after_limit_plus_one_bytes() {
        let url = "https://example.test/source.dic";
        let mut oversized = Cursor::new(b"abcdef");

        let error = read_response_with_limit(url, &mut oversized, 2)
            .expect_err("the third byte proves that the two-byte limit was exceeded");

        assert!(matches!(
            error,
            FetchError::FileTooLarge {
                limit: 2,
                actual: 3,
                ..
            }
        ));
        assert_eq!(oversized.position(), 3);

        let mut accepted = Cursor::new(b"abcdef");
        assert_eq!(
            read_response_with_limit(url, &mut accepted, 6)
                .expect("a response exactly at the limit is accepted"),
            b"abcdef"
        );
    }

    fn manifest() -> VerifiedDictionary {
        let aff = VerifiedFile::new("sample.aff", "https://example.test/sample.aff", SHA256_ABC)
            .expect("fixture aff is valid");
        let dic = VerifiedFile::new("sample.dic", "https://example.test/sample.dic", SHA256_ABC)
            .expect("fixture dic is valid");
        VerifiedDictionary::new(
            "en_US",
            "fixture-revision",
            "GPL-2.0-only",
            "fixture license",
            "https://example.test/license",
            aff,
            dic,
        )
        .expect("fixture manifest is valid")
    }

    #[derive(Debug)]
    struct FixtureFetcher {
        responses: BTreeMap<String, Result<Vec<u8>, String>>,
    }

    impl FixtureFetcher {
        fn from(items: impl IntoIterator<Item = (String, Result<Vec<u8>, String>)>) -> Self {
            Self {
                responses: items.into_iter().collect(),
            }
        }
    }

    impl Fetcher for FixtureFetcher {
        fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.responses
                .get(url)
                .ok_or_else(|| FetchError::Transport(format!("no fixture for {url}")))?
                .clone()
                .map_err(FetchError::Transport)
        }
    }

    struct LimitRecordingFetcher {
        requested_limit: Cell<Option<usize>>,
    }

    impl Fetcher for LimitRecordingFetcher {
        fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
            Ok(b"abc".to_vec())
        }

        fn fetch_with_limit(
            &self,
            url: &str,
            maximum_file_bytes: usize,
        ) -> Result<Vec<u8>, FetchError> {
            self.requested_limit.set(Some(maximum_file_bytes));
            enforce_response_limit(url, self.fetch(url)?, maximum_file_bytes)
        }
    }

    struct RacingFetcher {
        destination: PathBuf,
        competing_bytes: Vec<u8>,
    }

    impl Fetcher for RacingFetcher {
        fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
            fs::write(&self.destination, &self.competing_bytes).map_err(|source| {
                FetchError::WriteCache {
                    path: self.destination.clone(),
                    source,
                }
            })?;
            Ok(b"abc".to_vec())
        }
    }

    struct Cache(PathBuf);

    impl Cache {
        fn new() -> Self {
            let sequence = NEXT_CACHE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ferrolex-dictionaries-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("temporary cache root is writable");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Cache {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn public_catalog_entries_remain_copyable() {
        let _: LibreOfficeDictionary = LIBREOFFICE_CATALOG[0];
    }
}
