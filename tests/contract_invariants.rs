const CONTRACT: &str = include_str!("../circle/source/octra_sqlite_circle.c");

#[test]
fn octra_host_import_surface_stays_minimal() {
    let mut imports = CONTRACT
        .lines()
        .filter_map(|line| line.split("import_name(\"").nth(1))
        .filter_map(|tail| tail.split('"').next())
        .collect::<Vec<_>>();
    imports.sort_unstable();
    assert_eq!(
        imports,
        vec![
            "host_emit_event",
            "host_kv_del",
            "host_kv_get",
            "host_kv_get_len",
            "host_kv_put",
            "host_response_finish",
            "host_response_reset",
            "host_response_write",
        ]
    );
}

#[test]
fn sqlite_now_clock_is_deterministic_and_consistent() {
    let day = define_value("FIXED_JULIAN_DAY").parse::<f64>().unwrap();
    let ms = define_value("FIXED_JULIAN_MS")
        .trim_end_matches("ll")
        .parse::<i64>()
        .unwrap();
    assert_eq!(ms, (day * 86400000.0) as i64);
    assert!(CONTRACT.contains("*out_time = FIXED_JULIAN_DAY;"));
    assert!(CONTRACT.contains("*out_time = FIXED_JULIAN_MS;"));
}

#[test]
fn allocator_and_calloc_guard_overflow() {
    assert!(CONTRACT.contains("if (n == 0) n = 1;"));
    assert!(CONTRACT.contains("n + 8u > (usize)sizeof(heap) - aligned"));
    assert!(CONTRACT.contains("size > ((usize)~(usize)0) / count"));
}

#[test]
fn main_database_readonly_is_enforced_in_vfs() {
    assert!(CONTRACT.contains("if (f->is_main && f->readonly) return SQLITE_READONLY;"));
    assert!(CONTRACT.contains("if (f->readonly) return SQLITE_READONLY;"));
    assert!(CONTRACT.contains("octra_file->readonly = (flags & SQLITE_OPEN_READONLY) ? 1 : 0;"));
}

#[test]
fn dirty_page_buffer_remains_as_sqlite_write_stage() {
    assert!(CONTRACT.contains("#define MAX_DIRTY_PAGES 1024"));
    assert!(CONTRACT.contains("dirty_pages"));
    assert!(CONTRACT.contains("flush_dirty_pages"));
    assert!(CONTRACT.contains("meta_magic_v2"));
    assert!(CONTRACT.contains("meta_magic_v3"));
    assert!(CONTRACT.contains("make_gen_page_key"));
    assert!(CONTRACT.contains("make_manifest_key"));
}

fn define_value(name: &str) -> &'static str {
    CONTRACT
        .lines()
        .find_map(|line| line.strip_prefix(&format!("#define {name} ")))
        .unwrap_or_else(|| panic!("missing define {name}"))
}
