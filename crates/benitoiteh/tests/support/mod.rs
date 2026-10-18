//! 統合テストの提供者のネットワークを置き換える（設計書 07-01「模擬の提供者」）。

#[allow(dead_code)]
pub mod import_decoy;
pub mod mock_provider;

mod processor;
#[allow(unused_imports)]
pub use processor::benitoite;
mod skill;
#[allow(unused_imports)]
pub use skill::skills;

mod check;
#[allow(unused_imports)]
pub use check::checks;
