# Changelog

## [0.5.0](https://github.com/sebastian-software/ferrolex/compare/ferrolex-v0.4.0...ferrolex-v0.5.0) (2026-09-05)


### Features

* **cli:** report progress for long operations ([#187](https://github.com/sebastian-software/ferrolex/issues/187)) ([e9c9c0f](https://github.com/sebastian-software/ferrolex/commit/e9c9c0f0082c8a598cae664198f61877a2475fdc))
* compose layered candidate sources ([#235](https://github.com/sebastian-software/ferrolex/issues/235)) ([677748a](https://github.com/sebastian-software/ferrolex/commit/677748a8df4ad90c9a9b006a176cc012f870f094))
* expose product APIs from umbrella crate ([#234](https://github.com/sebastian-software/ferrolex/issues/234)) ([03ede49](https://github.com/sebastian-software/ferrolex/commit/03ede49a2118ceba7a51f540ddfd6a80f1da269f))
* **hunspell:** add owned dictionary extraction ([#232](https://github.com/sebastian-software/ferrolex/issues/232)) ([8c9e24a](https://github.com/sebastian-software/ferrolex/commit/8c9e24ae604d87b59345cd210fe980fa53636205))
* **hunspell:** provide configured suggester ([#233](https://github.com/sebastian-software/ferrolex/issues/233)) ([86d843c](https://github.com/sebastian-software/ferrolex/commit/86d843c502f10c00660d2085bef417f4865a8470))
* **node:** complete managed package contract ([#189](https://github.com/sebastian-software/ferrolex/issues/189)) ([7c7939c](https://github.com/sebastian-software/ferrolex/commit/7c7939c4d8a4944173bc82cbf045e6317ed83733))


### Bug Fixes

* **check:** align unicode normalization across entry points ([#224](https://github.com/sebastian-software/ferrolex/issues/224)) ([b0d3863](https://github.com/sebastian-software/ferrolex/commit/b0d386305c0f3090c5dea3b4c0380bdaf81b1c26))
* **cli:** align frequency word list handling ([#226](https://github.com/sebastian-software/ferrolex/issues/226)) ([172a420](https://github.com/sebastian-software/ferrolex/commit/172a42088975648366128108258c5ea6d1d82ff5))
* **core:** preserve user dictionary round trips ([#190](https://github.com/sebastian-software/ferrolex/issues/190)) ([#223](https://github.com/sebastian-software/ferrolex/issues/223)) ([319fd9f](https://github.com/sebastian-software/ferrolex/commit/319fd9f8b4837be9c65a994ae6c33c7d522e1234))
* expose catalog encoding import policy ([#236](https://github.com/sebastian-software/ferrolex/issues/236)) ([2455fff](https://github.com/sebastian-software/ferrolex/commit/2455fff7c079b704a9a3f333323d02c58b7eeb42))
* **hunspell:** allow capitalized checksharps keepcase forms ([#228](https://github.com/sebastian-software/ferrolex/issues/228)) ([d9cc1c9](https://github.com/sebastian-software/ferrolex/commit/d9cc1c913693b6dcef98c0f66fad2283c7faf646))
* **hunspell:** re-export public API result types ([#231](https://github.com/sebastian-software/ferrolex/issues/231)) ([345dbf0](https://github.com/sebastian-software/ferrolex/commit/345dbf096477d3cfa93cba9325aeb000aa02ccde))
* **hunspell:** skip ignored counted lines ([#183](https://github.com/sebastian-software/ferrolex/issues/183)) ([f6155f5](https://github.com/sebastian-software/ferrolex/commit/f6155f5c284a8a3b99192f870b2a132f0f9bbe4c))
* **io:** harden dictionary write durability ([#182](https://github.com/sebastian-software/ferrolex/issues/182)) ([029cfc2](https://github.com/sebastian-software/ferrolex/commit/029cfc2632ed5ff9629766f57c0c437585d79cca))
* normalize leading BOMs in Hunspell sources ([#179](https://github.com/sebastian-software/ferrolex/issues/179)) ([568b557](https://github.com/sebastian-software/ferrolex/commit/568b557ba66ba7712bd6413d15a3cec300dfcb39))
* relax library dependency pins ([#237](https://github.com/sebastian-software/ferrolex/issues/237)) ([d43940b](https://github.com/sebastian-software/ferrolex/commit/d43940bff00a8e50d137e858ebfcd80c46ad9202))
* render actionable import diagnostics ([#238](https://github.com/sebastian-software/ferrolex/issues/238)) ([5fecb30](https://github.com/sebastian-software/ferrolex/commit/5fecb3021485194652be89f0b26c13c346cc88d4))
* repair 0.4.0 workspace release contract ([#180](https://github.com/sebastian-software/ferrolex/issues/180)) ([d662d49](https://github.com/sebastian-software/ferrolex/commit/d662d49aa5fa3f4bec86e5ce36bca0c86f09cd4b))
* **suggest:** preserve valid suggestion casing ([#225](https://github.com/sebastian-software/ferrolex/issues/225)) ([37b430c](https://github.com/sebastian-software/ferrolex/commit/37b430c1b8e710d45fe2b25bf491a9f21224d033))
* **text:** ignore digit-adjacent word fragments ([#227](https://github.com/sebastian-software/ferrolex/issues/227)) ([d6e8096](https://github.com/sebastian-software/ferrolex/commit/d6e809641c9aaade9f873a8e2968149986bf8ee5))


### Performance Improvements

* **hunspell:** bound empty-add miss lookup ([#185](https://github.com/sebastian-software/ferrolex/issues/185)) ([6c4b927](https://github.com/sebastian-software/ferrolex/commit/6c4b9275fe8f6adc16a343a238923f4b1d4553c1))
* **hunspell:** reduce lookup and import allocations ([#186](https://github.com/sebastian-software/ferrolex/issues/186)) ([afa9491](https://github.com/sebastian-software/ferrolex/commit/afa9491e55d6db5f19ef9674969aca3ca9c070e1))
* remove redundant artifact loading work ([#184](https://github.com/sebastian-software/ferrolex/issues/184)) ([147d5f2](https://github.com/sebastian-software/ferrolex/commit/147d5f2702aeacad049090064466bcca6db1ee2d))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ferrolex-core bumped from 0.4.0 to 0.4.1
    * ferrolex-dictionaries bumped from 0.4.0 to 0.4.1
    * ferrolex-hunspell bumped from 0.4.0 to 0.4.1
    * ferrolex-suggest bumped from 0.4.0 to 0.4.1

## [0.4.0](https://github.com/sebastian-software/ferrolex/compare/ferrolex-v0.3.0...ferrolex-v0.4.0) (2026-09-02)


### Features

* **cli:** add focused command help ([#154](https://github.com/sebastian-software/ferrolex/issues/154)) ([ae1c651](https://github.com/sebastian-software/ferrolex/commit/ae1c6516852b7f1568cf64e1ca64abdcf45ed06d))
* **cli:** add JSON Lines output ([#166](https://github.com/sebastian-software/ferrolex/issues/166)) ([16b83b4](https://github.com/sebastian-software/ferrolex/commit/16b83b401e0660a93e978fe5a981e19f6788c0ab))
* **cli:** improve check input ergonomics ([#165](https://github.com/sebastian-software/ferrolex/issues/165)) ([bd15be3](https://github.com/sebastian-software/ferrolex/commit/bd15be3c642e33977cde9fe68a2d998471883f77))


### Bug Fixes

* **ci:** satisfy current clippy ([#151](https://github.com/sebastian-software/ferrolex/issues/151)) ([56376e6](https://github.com/sebastian-software/ferrolex/commit/56376e6a8a9938a958aa59bbb3787ed3be81b0df))
* **cli:** clarify process contract ([#153](https://github.com/sebastian-software/ferrolex/issues/153)) ([d28799c](https://github.com/sebastian-software/ferrolex/commit/d28799c57867455810cd72aaa70bdcdb867e8732))
* **cli:** import cacheless Hunspell sources ([#159](https://github.com/sebastian-software/ferrolex/issues/159)) ([5ca3b81](https://github.com/sebastian-software/ferrolex/commit/5ca3b81725a031545b4add34dd58df9da120d714))
* **cli:** layer suggestion dictionaries ([#158](https://github.com/sebastian-software/ferrolex/issues/158)) ([f3e537f](https://github.com/sebastian-software/ferrolex/commit/f3e537f346242b8ac381f3cb7cebd1ca0c761799))
* **cli:** load persisted user dictionaries ([#164](https://github.com/sebastian-software/ferrolex/issues/164)) ([8d1af2d](https://github.com/sebastian-software/ferrolex/commit/8d1af2d720ab348463073d8acf4ebb5a921d27cd))
* **cli:** make directory analysis resilient ([#155](https://github.com/sebastian-software/ferrolex/issues/155)) ([191f62b](https://github.com/sebastian-software/ferrolex/commit/191f62ba136e12676c5ec15db0e2ba0feb701d46))
* **dictionaries:** accept identical concurrent installs ([#167](https://github.com/sebastian-software/ferrolex/issues/167)) ([5a54d99](https://github.com/sebastian-software/ferrolex/commit/5a54d990f866e8d68214dc5d2269e008b2246c5b))
* **dictionaries:** report refused redirects ([#168](https://github.com/sebastian-software/ferrolex/issues/168)) ([d9e4258](https://github.com/sebastian-software/ferrolex/commit/d9e4258722b25ed2b660b02d55752f4280682d92))
* honor installer file-size limits during fetch ([#178](https://github.com/sebastian-software/ferrolex/issues/178)) ([4ca9947](https://github.com/sebastian-software/ferrolex/commit/4ca994730bf59832915e6b32f888988793b22a39))
* **hunspell:** bound compound explanation tracing ([#157](https://github.com/sebastian-software/ferrolex/issues/157)) ([a22c964](https://github.com/sebastian-software/ferrolex/commit/a22c9647ba38e2b8e69925502f391ad0962da762))
* preserve live dictionary visibility ([f0e779e](https://github.com/sebastian-software/ferrolex/commit/f0e779ef4d93a316534e2c327ec500ddfae59dbb))
* **release:** enforce workspace version contract ([#160](https://github.com/sebastian-software/ferrolex/issues/160)) ([df3f4a6](https://github.com/sebastian-software/ferrolex/commit/df3f4a61cd0dd65fbab378b685ee14514e03a7c0))
* **release:** publish crates after compatibility checks ([#163](https://github.com/sebastian-software/ferrolex/issues/163)) ([e885b4f](https://github.com/sebastian-software/ferrolex/commit/e885b4fc25a0694d27f4fd418ecaa0e8c8548276))
* **suggest:** bound related seed normalization ([1c3bb35](https://github.com/sebastian-software/ferrolex/commit/1c3bb35043e7d34c282763d3ca09d1c458175bd8))
* **suggest:** bound related seed normalization ([fa8bb26](https://github.com/sebastian-software/ferrolex/commit/fa8bb26473bb95591161aeb6de782e6b48720978))
* **suggest:** index query-related candidates ([#156](https://github.com/sebastian-software/ferrolex/issues/156)) ([d5cedc1](https://github.com/sebastian-software/ferrolex/commit/d5cedc1f73d7e2a1fcda837ab17356faf12ff1fa))
* **suggest:** report skipped related seeds ([1cf52db](https://github.com/sebastian-software/ferrolex/commit/1cf52dbe690fe1a61a5c3a2ee4d5e49f41b2dcd1))


### Performance Improvements

* cache normalized token recognition ([8c55415](https://github.com/sebastian-software/ferrolex/commit/8c55415253c84cdcf9fef30d2fd20253e21a9359))
* compact hunspell runtime state ([3245c6b](https://github.com/sebastian-software/ferrolex/commit/3245c6b74241197d1df01c24cdf00bbd599d058e))
* compact Hunspell runtime state ([ffb8c80](https://github.com/sebastian-software/ferrolex/commit/ffb8c80f23ece31f268135ab5c9991ccbf0dc97e))
* reuse analyze suggestion state ([0dec48d](https://github.com/sebastian-software/ferrolex/commit/0dec48dd8af6d7de140226d7842ac4c2a3414293))
* reuse analyze suggestion state ([53945ec](https://github.com/sebastian-software/ferrolex/commit/53945ecc5a85926fd42e8ae10770a21270394c90))
* skip duplicate normalized token lookups ([a01be21](https://github.com/sebastian-software/ferrolex/commit/a01be217c718a10491c55d23b32c0abbfbc765f9))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * ferrolex-core bumped from 0.3.0 to 0.3.1

## [0.3.0](https://github.com/sebastian-software/ferrolex/compare/ferrolex-v0.2.0...ferrolex-v0.3.0) (2026-08-14)


### Features

* **code:** add parser-backed Rust analysis ([51e2016](https://github.com/sebastian-software/ferrolex/commit/51e2016d7fc39fc75d544b34e9ab014ed03de2c2))


### Bug Fixes

* **ci:** restore cargo policy baseline ([8b18d02](https://github.com/sebastian-software/ferrolex/commit/8b18d020ec4519d19c417db3900b55b03178501d))
* **cli:** preserve explicit Rust comment overrides ([a71e5ad](https://github.com/sebastian-software/ferrolex/commit/a71e5ad1bef335bac46f38e1906f866c79632db8))
* **code:** exclude crate macro metavariable ([19bacb6](https://github.com/sebastian-software/ferrolex/commit/19bacb61a1cd76ec4cedd0409e839f515cdf93af))
* **code:** preserve Rust analysis contracts ([b2c2ae1](https://github.com/sebastian-software/ferrolex/commit/b2c2ae15250c811c0dc908e71ed3e519f155e824))
* **hunspell:** bind scorecard corpus identity ([cac8384](https://github.com/sebastian-software/ferrolex/commit/cac8384a14e5baeed85e4e84c9167c0b56e26747))
* **hunspell:** harden scorecard evidence ([d2d8766](https://github.com/sebastian-software/ferrolex/commit/d2d8766f6512d0df1625de1c5c70868b4eb746f1))

## [0.2.0](https://github.com/sebastian-software/ferrolex/releases/tag/ferrolex-v0.2.0) (2026-08-13)


### Features

* add experimental C ABI spike ([220c1bb](https://github.com/sebastian-software/ferrolex/commit/220c1bba010f39526c6beb552bdccd5ad1292bfd))
* add generic stdio language server ([72d84a0](https://github.com/sebastian-software/ferrolex/commit/72d84a05a320888eed231befec63eadf8efeca54))
* add Hunspell explanation CLI ([f133cea](https://github.com/sebastian-software/ferrolex/commit/f133cea9d172af220024299421c39ce8cc8fb274))
* add Node and Python binding spikes ([8433506](https://github.com/sebastian-software/ferrolex/commit/84335067939dd5128f7d6b0aee454ab8bd3feabb))
* add priority locale compatibility fixtures ([ef92a52](https://github.com/sebastian-software/ferrolex/commit/ef92a52d4e14aa24faad7a323dd9f5bc474cab0c))
* add VS Code language client ([4ed97a1](https://github.com/sebastian-software/ferrolex/commit/4ed97a179d30a2469f14178a3303134a0abab6f5))
* explain Hunspell lookup decisions ([1a1f783](https://github.com/sebastian-software/ferrolex/commit/1a1f783ce694e28d0b4f9ad5774a625b1672b059))
* record dictionary SPDX license evidence ([4893f3a](https://github.com/sebastian-software/ferrolex/commit/4893f3a01aed852bd676e13bacda80411504001d))


### Bug Fixes

* bound dictionary fetching and cache installation ([a23f89d](https://github.com/sebastian-software/ferrolex/commit/a23f89de83342b674766be186f8d9c6362566400))
* modernize Hunspell code for Rust 1.88 ([b14b0a3](https://github.com/sebastian-software/ferrolex/commit/b14b0a3027f01089fa0c548edf5fe9268d92a07f))
* **release:** restrict umbrella package contents ([cce6132](https://github.com/sebastian-software/ferrolex/commit/cce61329be0e86f0f85034e9bc3a313479264681))
