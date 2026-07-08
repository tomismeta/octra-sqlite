pub(super) struct HistoricalWasm {
    pub(super) releases: &'static str,
    pub(super) sqlite_version: &'static str,
    pub(super) sha256: &'static str,
    pub(super) bytes: u64,
    pub(super) source_url: &'static str,
}

pub(super) const HISTORICAL_WASMS: &[HistoricalWasm] = &[
    HistoricalWasm {
        releases: "0.1.0-0.2.0",
        sqlite_version: "3.53.2",
        sha256: "f6df77206d82bcfdb07cbd7f2d6eaebc21636add7f41c114d78b15eb16bdc7cf",
        bytes: 607_640,
        source_url: "https://raw.githubusercontent.com/tomismeta/octra-sqlite/v0.2.0/circle/wasm/octra_sqlite_circle.wasm",
    },
    HistoricalWasm {
        releases: "0.2.1",
        sqlite_version: "3.53.2",
        sha256: "29861d38ddad25f5cd2b153bb70cfa6b1b54ebd2532fe931fa1f012b7f39ca9c",
        bytes: 607_800,
        source_url: "https://raw.githubusercontent.com/tomismeta/octra-sqlite/v0.2.1/circle/wasm/octra_sqlite_circle.wasm",
    },
    HistoricalWasm {
        releases: "0.3.0",
        sqlite_version: "3.53.2",
        sha256: "8158f507a349cec2a97993d513ca2d3b275d9aaf4e39ea1edee414ce55d415ea",
        bytes: 609_475,
        source_url: "https://raw.githubusercontent.com/tomismeta/octra-sqlite/v0.3.0/circle/wasm/octra_sqlite_circle.wasm",
    },
    HistoricalWasm {
        releases: "0.3.1-0.3.2",
        sqlite_version: "3.53.2",
        sha256: "39635962bffb470daced92396ee27e206e6b3ea000b4ec7a954d3bcd05ba662b",
        bytes: 609_404,
        source_url: "https://raw.githubusercontent.com/tomismeta/octra-sqlite/v0.3.2/circle/wasm/octra_sqlite_circle.wasm",
    },
    HistoricalWasm {
        releases: "0.3.3-0.5.2",
        sqlite_version: "3.53.2",
        sha256: "36664d04fd0457c4c7da200328c753984746769cec479fd93f799665c66f8c5d",
        bytes: 609_354,
        source_url: "https://raw.githubusercontent.com/tomismeta/octra-sqlite/v0.5.2/circle/wasm/octra_sqlite_circle.wasm",
    },
];
