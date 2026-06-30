#[cfg(feature = "wasm-behavior")]
mod wasm_behavior {
    use anyhow::{anyhow, bail, Result};
    use base64::{engine::general_purpose, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command as ProcessCommand;
    use std::rc::Rc;
    use wasmtime::{Caller, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

    const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
    const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
    const OSW1_DOMAIN: &[u8] = b"octra-sqlite.osw1.v1\0";

    #[derive(Default)]
    struct Host {
        kv: BTreeMap<Vec<u8>, Vec<u8>>,
        response: Vec<u8>,
        status: i32,
        events: Vec<(String, String)>,
        put_count: usize,
        del_count: usize,
        fail_put_after: Option<usize>,
        fail_put_key_contains: Option<String>,
    }

    struct Contract {
        store: Store<Rc<RefCell<Host>>>,
        instance: Instance,
        alloc: TypedFunc<i32, i32>,
        query: TypedFunc<(i32, i32), i32>,
        update: TypedFunc<(i32, i32), i32>,
        host: Rc<RefCell<Host>>,
    }

    impl Contract {
        fn load() -> Result<Self> {
            let wasm = std::env::var("OCTRA_SQLITE_WASM")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("circle/wasm/octra_sqlite_circle.wasm"));
            let engine = Engine::default();
            let module = Module::from_file(&engine, &wasm)
                .map_err(|error| anyhow!("load {}: {error}", wasm.display()))?;
            Self::instantiate(engine, module)
        }

        fn load_patched(owner_pubkey: &[u8; 32], db_id: &[u8; 32]) -> Result<Self> {
            let wasm = std::env::var("OCTRA_SQLITE_WASM")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("circle/wasm/octra_sqlite_circle.wasm"));
            let mut bytes =
                fs::read(&wasm).map_err(|error| anyhow!("read {}: {error}", wasm.display()))?;
            replace_placeholder(&mut bytes, OWNER_PUBKEY_PLACEHOLDER, owner_pubkey)?;
            replace_placeholder(&mut bytes, DB_ID_PLACEHOLDER, db_id)?;
            let engine = Engine::default();
            let module = Module::new(&engine, bytes)?;
            Self::instantiate(engine, module)
        }

        fn instantiate(engine: Engine, module: Module) -> Result<Self> {
            let mut imports = module
                .imports()
                .map(|import| format!("{}.{}", import.module(), import.name()))
                .collect::<Vec<_>>();
            imports.sort();
            let mut expected_imports = [
                "octra.host_response_reset",
                "octra.host_response_write",
                "octra.host_response_finish",
                "octra.host_kv_get_len",
                "octra.host_kv_get",
                "octra.host_kv_put",
                "octra.host_kv_del",
                "octra.host_emit_event",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
            expected_imports.sort();
            assert_eq!(imports, expected_imports);
            let host = Rc::new(RefCell::new(Host::default()));
            let mut store = Store::new(&engine, host.clone());
            let mut linker = Linker::new(&engine);
            add_host_imports(&mut linker)?;
            let instance = linker.instantiate(&mut store, &module)?;
            let alloc = instance.get_typed_func::<i32, i32>(&mut store, "octra_alloc")?;
            let query = instance.get_typed_func::<(i32, i32), i32>(&mut store, "octra_query")?;
            let update = instance.get_typed_func::<(i32, i32), i32>(&mut store, "octra_update")?;
            Ok(Self {
                store,
                instance,
                alloc,
                query,
                update,
                host,
            })
        }

        fn call_query(&mut self, method: &str, params: &[&str]) -> Result<String> {
            let frame = call_frame(method, params);
            let ptr = self.alloc.call(&mut self.store, frame.len() as i32)?;
            memory(&mut self.store, &self.instance)?.write(
                &mut self.store,
                ptr as usize,
                &frame,
            )?;
            let rc = self
                .query
                .call(&mut self.store, (ptr, frame.len() as i32))?;
            let decoded = decode_response(&self.host.borrow().response);
            if rc != 0 && decoded.is_err() {
                bail!("query {method} returned {rc}");
            }
            decoded
        }

        fn call_update(&mut self, method: &str, params: &[&str]) -> Result<String> {
            let frame = call_frame(method, params);
            self.call_raw_update(&frame)
        }

        fn call_raw_query(&mut self, frame: &[u8]) -> Result<(i32, Option<String>)> {
            let ptr = self.alloc.call(&mut self.store, frame.len() as i32)?;
            memory(&mut self.store, &self.instance)?.write(&mut self.store, ptr as usize, frame)?;
            let rc = self
                .query
                .call(&mut self.store, (ptr, frame.len() as i32))?;
            Ok((rc, decode_response(&self.host.borrow().response).ok()))
        }

        fn call_raw_update(&mut self, frame: &[u8]) -> Result<String> {
            let ptr = self.alloc.call(&mut self.store, frame.len() as i32)?;
            memory(&mut self.store, &self.instance)?.write(&mut self.store, ptr as usize, frame)?;
            let rc = self
                .update
                .call(&mut self.store, (ptr, frame.len() as i32))?;
            let decoded = decode_response(&self.host.borrow().response);
            if rc != 0 && decoded.is_err() {
                bail!("update returned {rc}");
            }
            decoded
        }
    }

    fn replace_placeholder(bytes: &mut [u8], placeholder: &[u8], replacement: &[u8]) -> Result<()> {
        let positions = bytes
            .windows(placeholder.len())
            .enumerate()
            .filter_map(|(index, window)| (window == placeholder).then_some(index))
            .collect::<Vec<_>>();
        if positions.len() != 1 {
            bail!(
                "expected one placeholder occurrence, found {} for {:?}",
                positions.len(),
                String::from_utf8_lossy(placeholder)
            );
        }
        let start = positions[0];
        bytes[start..start + replacement.len()].copy_from_slice(replacement);
        Ok(())
    }

    fn add_host_imports(linker: &mut Linker<Rc<RefCell<Host>>>) -> Result<()> {
        linker.func_wrap(
            "octra",
            "host_response_reset",
            |caller: Caller<'_, Rc<RefCell<Host>>>| -> i32 {
                let mut host = caller.data().borrow_mut();
                host.response.clear();
                host.status = 0;
                0
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_response_write",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             ptr: i32,
             len: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let bytes = read_memory(&mut caller, ptr, len)?;
                caller.data().borrow_mut().response.extend(bytes);
                Ok::<i32, wasmtime::Error>(0)
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_response_finish",
            |caller: Caller<'_, Rc<RefCell<Host>>>, status: i32| -> i32 {
                caller.data().borrow_mut().status = status;
                0
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_kv_get_len",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             ptr: i32,
             len: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let key = read_memory(&mut caller, ptr, len)?;
                Ok::<i32, wasmtime::Error>(
                    caller
                        .data()
                        .borrow()
                        .kv
                        .get(&key)
                        .map(|value| value.len() as i32)
                        .unwrap_or(-1),
                )
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_kv_get",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             key_ptr: i32,
             key_len: i32,
             out_ptr: i32,
             out_cap: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let key = read_memory(&mut caller, key_ptr, key_len)?;
                let value = caller.data().borrow().kv.get(&key).cloned();
                if let Some(value) = value {
                    if value.len() > out_cap as usize {
                        return Ok::<i32, wasmtime::Error>(-2);
                    }
                    write_memory(&mut caller, out_ptr, &value)?;
                    Ok(value.len() as i32)
                } else {
                    Ok(-1)
                }
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_kv_put",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             key_ptr: i32,
             key_len: i32,
             value_ptr: i32,
             value_len: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let key = read_memory(&mut caller, key_ptr, key_len)?;
                let value = read_memory(&mut caller, value_ptr, value_len)?;
                let mut host = caller.data().borrow_mut();
                host.put_count += 1;
                if host
                    .fail_put_after
                    .is_some_and(|limit| host.put_count > limit)
                {
                    return Ok::<i32, wasmtime::Error>(-1);
                }
                if host
                    .fail_put_key_contains
                    .as_ref()
                    .is_some_and(|needle| String::from_utf8_lossy(&key).contains(needle))
                {
                    return Ok::<i32, wasmtime::Error>(-1);
                }
                host.kv.insert(key, value);
                Ok(0)
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_kv_del",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             key_ptr: i32,
             key_len: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let key = read_memory(&mut caller, key_ptr, key_len)?;
                let mut host = caller.data().borrow_mut();
                host.del_count += 1;
                host.kv.remove(&key);
                Ok::<i32, wasmtime::Error>(0)
            },
        )?;
        linker.func_wrap(
            "octra",
            "host_emit_event",
            |mut caller: Caller<'_, Rc<RefCell<Host>>>,
             topic_ptr: i32,
             topic_len: i32,
             data_ptr: i32,
             data_len: i32|
             -> std::result::Result<i32, wasmtime::Error> {
                let topic =
                    String::from_utf8_lossy(&read_memory(&mut caller, topic_ptr, topic_len)?)
                        .to_string();
                let data = String::from_utf8_lossy(&read_memory(&mut caller, data_ptr, data_len)?)
                    .to_string();
                caller.data().borrow_mut().events.push((topic, data));
                Ok::<i32, wasmtime::Error>(0)
            },
        )?;
        Ok(())
    }

    fn memory(store: &mut Store<Rc<RefCell<Host>>>, instance: &Instance) -> Result<Memory> {
        instance
            .get_memory(store, "memory")
            .ok_or_else(|| anyhow!("missing memory export"))
    }

    fn caller_memory(
        caller: &mut Caller<'_, Rc<RefCell<Host>>>,
    ) -> Result<Memory, wasmtime::Error> {
        caller
            .get_export("memory")
            .and_then(|export| export.into_memory())
            .ok_or_else(|| wasmtime::Error::msg("missing memory export"))
    }

    fn read_memory(
        caller: &mut Caller<'_, Rc<RefCell<Host>>>,
        ptr: i32,
        len: i32,
    ) -> Result<Vec<u8>, wasmtime::Error> {
        let memory = caller_memory(caller)?;
        let mut bytes = vec![0u8; len as usize];
        memory.read(caller, ptr as usize, &mut bytes)?;
        Ok(bytes)
    }

    fn write_memory(
        caller: &mut Caller<'_, Rc<RefCell<Host>>>,
        ptr: i32,
        bytes: &[u8],
    ) -> Result<(), wasmtime::Error> {
        let memory = caller_memory(caller)?;
        memory.write(caller, ptr as usize, bytes)?;
        Ok(())
    }

    fn call_frame(method: &str, params: &[&str]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"OCWR1");
        frame.extend_from_slice(&(method.len() as u16).to_be_bytes());
        frame.extend_from_slice(method.as_bytes());
        frame.extend_from_slice(&(params.len() as u16).to_be_bytes());
        for param in params {
            frame.push(4);
            frame.extend_from_slice(&(param.len() as u32).to_be_bytes());
            frame.extend_from_slice(param.as_bytes());
        }
        frame
    }

    fn decode_response(frame: &[u8]) -> Result<String> {
        if frame.len() < 10 || &frame[..5] != b"OCWS1" || frame[5] != 4 {
            bail!("bad response frame: {frame:?}");
        }
        let len = u32::from_be_bytes(frame[6..10].try_into().unwrap()) as usize;
        if frame.len() != 10 + len {
            bail!("bad response frame length");
        }
        Ok(String::from_utf8_lossy(&frame[10..]).to_string())
    }

    fn json_response(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn osw1_frame(db_id: &[u8; 32], sequence: u64, method: &str, sql: &str) -> Vec<u8> {
        let mut msg =
            Vec::with_capacity(OSW1_DOMAIN.len() + 32 + 8 + 2 + method.len() + 4 + sql.len());
        msg.extend_from_slice(OSW1_DOMAIN);
        msg.extend_from_slice(db_id);
        msg.extend_from_slice(&sequence.to_be_bytes());
        msg.extend_from_slice(&(method.len() as u16).to_be_bytes());
        msg.extend_from_slice(method.as_bytes());
        msg.extend_from_slice(&(sql.len() as u32).to_be_bytes());
        msg.extend_from_slice(sql.as_bytes());
        msg
    }

    fn sign_osw1(
        key: &SigningKey,
        db_id: &[u8; 32],
        sequence: u64,
        method: &str,
        sql: &str,
    ) -> String {
        hex::encode(
            key.sign(&osw1_frame(db_id, sequence, method, sql))
                .to_bytes(),
        )
    }

    fn owner_fixture() -> (SigningKey, [u8; 32], [u8; 32]) {
        let owner = SigningKey::from_bytes(&[7u8; 32]);
        let owner_pubkey = owner.verifying_key().to_bytes();
        let db_id = Sha256::digest(b"test-db-id");
        let mut db_id_bytes = [0u8; 32];
        db_id_bytes.copy_from_slice(&db_id);
        (owner, owner_pubkey, db_id_bytes)
    }

    fn kv_keys(contract: &Contract) -> Vec<String> {
        contract
            .host
            .borrow()
            .kv
            .keys()
            .map(|key| String::from_utf8_lossy(key).to_string())
            .collect()
    }

    fn gen_page_key_count(contract: &Contract) -> usize {
        kv_keys(contract)
            .into_iter()
            .filter(|key| key.starts_with("octra.sqlite.vfs.v1.gen.") && key.contains(".page."))
            .count()
    }

    fn manifest_key_count(contract: &Contract) -> usize {
        kv_keys(contract)
            .into_iter()
            .filter(|key| key.starts_with("octra.sqlite.vfs.v1.gen.") && key.ends_with(".manifest"))
            .count()
    }

    #[test]
    fn osw1_binary_golden_vector() -> Result<()> {
        let (_, _, db_id) = owner_fixture();
        let msg = osw1_frame(&db_id, 42, "exec", "select 1;");
        assert_eq!(
            hex::encode(msg),
            "6f637472612d73716c6974652e6f7377312e7631001fce55ad53f355909514a6a349e2afb2a22cf3bca124d239a9ace46a4108c482000000000000002a0004657865630000000973656c65637420313b"
        );
        Ok(())
    }

    #[test]
    fn osw1_golden_vector_signature_is_accepted_by_contract() -> Result<()> {
        let (owner, owner_pubkey, db_id_bytes) = owner_fixture();
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;
        let sql = "select 1;";
        let sig = sign_osw1(&owner, &db_id_bytes, 42, "exec", sql);
        let pubkey = hex::encode(owner_pubkey);
        let accepted = json_response(&contract.call_update("exec", &[sql, &pubkey, "42", &sig])?);
        assert_eq!(accepted["ok"], true);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 42);
        Ok(())
    }

    #[test]
    fn auth_info_does_not_require_existing_storage_pages() -> Result<()> {
        let (_, owner_pubkey, db_id_bytes) = owner_fixture();
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;

        assert!(contract.host.borrow().kv.is_empty());
        let auth = json_response(&contract.call_query("auth_info", &[])?);

        assert_eq!(auth["ok"], true);
        assert_eq!(auth["configured"], true);
        assert_eq!(auth["auth"], "osw1");
        assert_eq!(auth["owner_pubkey"], hex::encode(owner_pubkey));
        assert_eq!(auth["db_id"], hex::encode(db_id_bytes));
        assert!(auth.get("owner_sequence").is_none());
        assert!(contract.host.borrow().kv.is_empty());
        Ok(())
    }

    #[test]
    fn commit_rollback_and_generation_storage_are_behavioral() -> Result<()> {
        let mut contract = Contract::load()?;
        let first = json_response(&contract.call_update(
            "exec",
            &[
                "create table people(first_name text not null, last_name text not null);
insert into people(first_name,last_name) values ('Ada','Byron'),('Katherine','Johnson');",
            ],
        )?);
        assert_eq!(first["ok"], true);
        assert_eq!(first["generation"], 1);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["commit_protocol"], "generation_manifest_v4");
        assert_eq!(storage["generation"], 1);
        assert_eq!(storage["owner_sequence"], 0);

        let before = contract.host.borrow().kv.clone();
        let failed = json_response(&contract.call_update(
            "exec",
            &[
                "insert into people(first_name,last_name) values ('Grace','Hopper');
select no_such_column from people;",
            ],
        )?);
        assert_eq!(failed["ok"], false);
        assert_eq!(contract.host.borrow().kv, before);

        let rows = contract.call_query(
            "query_typed",
            &["select first_name,last_name from people order by first_name;"],
        )?;
        assert!(rows.starts_with("OSR1:"));
        Ok(())
    }

    #[test]
    fn owner_patched_wasm_auth_matrix_and_atomic_sequence() -> Result<()> {
        let (owner, owner_pubkey, db_id_bytes) = owner_fixture();
        let other = SigningKey::from_bytes(&[8u8; 32]);
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;

        let unsigned = json_response(
            &contract.call_update("exec", &["create table person(first_name text not null);"])?,
        );
        assert_eq!(unsigned["ok"], false);
        assert_eq!(unsigned["error"], "auth_required");
        assert_eq!(contract.host.borrow().status, 401);
        assert!(contract
            .host
            .borrow()
            .events
            .iter()
            .any(|(topic, data)| topic == "octra.sqlite.auth"
                && data.contains("auth_not_authenticated:auth_required")));

        let reset = json_response(&contract.call_update("reset", &[])?);
        assert_eq!(reset["ok"], false);
        assert_eq!(reset["error"], "auth_required");

        let sql = "create table person(first_name text not null);
insert into person(first_name) values ('Ada');";
        let pubkey = hex::encode(owner_pubkey);

        let wrong_method_sig = sign_osw1(&owner, &db_id_bytes, 1, "exec", sql);
        let wrong_method = json_response(
            &contract.call_update("exec_trace", &[sql, &pubkey, "1", &wrong_method_sig])?,
        );
        assert_eq!(wrong_method["ok"], false);
        assert_eq!(wrong_method["error"], "auth_bad_signature");
        assert_eq!(contract.host.borrow().status, 401);

        let sig = sign_osw1(&owner, &db_id_bytes, 1, "exec_trace", sql);
        let written =
            json_response(&contract.call_update("exec_trace", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(written["ok"], true);
        assert!(contract
            .host
            .borrow()
            .events
            .iter()
            .any(
                |(topic, data)| topic == "octra.sqlite.sql" && data.contains("create table person")
            ));

        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 1);

        let replay =
            json_response(&contract.call_update("exec_trace", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(replay["ok"], false);
        assert_eq!(replay["error"], "auth_replay");

        let denied_sql = "insert into person(first_name) values ('Mallory');";
        let bad_sig = sign_osw1(&other, &db_id_bytes, 2, "exec", denied_sql);
        let bad_pubkey = hex::encode(other.verifying_key().to_bytes());
        let denied = json_response(
            &contract.call_update("exec", &[denied_sql, &bad_pubkey, "2", &bad_sig])?,
        );
        assert_eq!(denied["ok"], false);
        assert_eq!(denied["error"], "auth_denied");
        assert_eq!(contract.host.borrow().status, 403);
        assert!(contract
            .host
            .borrow()
            .events
            .iter()
            .any(|(topic, data)| topic == "octra.sqlite.auth"
                && data.contains("auth_not_authorized:auth_denied")));

        let mut wrong_db_id = [0u8; 32];
        wrong_db_id.copy_from_slice(&Sha256::digest(b"wrong-db-id"));
        let wrong_db_sig = sign_osw1(&owner, &wrong_db_id, 2, "exec", denied_sql);
        let wrong_db = json_response(
            &contract.call_update("exec", &[denied_sql, &pubkey, "2", &wrong_db_sig])?,
        );
        assert_eq!(wrong_db["ok"], false);
        assert_eq!(wrong_db["error"], "auth_bad_signature");

        let signed_sql = "insert into person(first_name) values ('Grace');";
        let tampered_sql = "insert into person(first_name) values ('Mallory');";
        let tampered_sig = sign_osw1(&owner, &db_id_bytes, 2, "exec", signed_sql);
        let tampered = json_response(
            &contract.call_update("exec", &[tampered_sql, &pubkey, "2", &tampered_sig])?,
        );
        assert_eq!(tampered["ok"], false);
        assert_eq!(tampered["error"], "auth_bad_signature");

        let good_sig = sign_osw1(&owner, &db_id_bytes, 2, "exec", signed_sql);
        let good =
            json_response(&contract.call_update("exec", &[signed_sql, &pubkey, "2", &good_sig])?);
        assert_eq!(good["ok"], true);

        let failing_sql = "insert into missing_table(first_name) values ('Nope');";
        let failing_sig = sign_osw1(&owner, &db_id_bytes, 3, "exec", failing_sql);
        let failed_sql = json_response(
            &contract.call_update("exec", &[failing_sql, &pubkey, "3", &failing_sig])?,
        );
        assert_eq!(failed_sql["ok"], false);
        assert_eq!(failed_sql["error"], "sqlite_exec_failed");
        assert_eq!(contract.host.borrow().status, 0);
        assert!(contract
            .host
            .borrow()
            .events
            .iter()
            .any(|(topic, data)| topic == "octra.sqlite.error"
                && data.contains("sqlite_exec_failed:no such table: missing_table")));
        let storage_after_failed_sql = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage_after_failed_sql["owner_sequence"], 2);

        let retry_sql = "insert into person(first_name) values ('Katherine');";
        let retry_sig = sign_osw1(&owner, &db_id_bytes, 3, "exec", retry_sql);
        let retry =
            json_response(&contract.call_update("exec", &[retry_sql, &pubkey, "3", &retry_sig])?);
        assert_eq!(retry["ok"], true);

        let rows = json_response(&contract.call_query(
            "query",
            &["select first_name from person order by first_name;"],
        )?);
        assert_eq!(rows["rows"][0][0], "Ada");
        assert_eq!(rows["rows"][1][0], "Grace");
        assert_eq!(rows["rows"][2][0], "Katherine");
        Ok(())
    }

    #[test]
    fn delta_generation_writes_do_not_copy_the_full_database_and_gc_old_pages() -> Result<()> {
        let mut contract = Contract::load()?;
        let seed = "\
create table pages(id integer primary key, payload text not null);
with recursive n(x) as (values(1) union all select x + 1 from n where x < 64)
insert into pages(payload) select hex(zeroblob(512)) from n;";
        let first = json_response(&contract.call_update("exec", &[seed])?);
        assert_eq!(first["ok"], true);

        let storage = json_response(&contract.call_query("storage_info", &[])?);
        let page_count = storage["page_count"].as_u64().unwrap();
        assert!(
            page_count > 8,
            "seed should create enough pages to distinguish sparse from full flush: {storage}"
        );
        assert_eq!(manifest_key_count(&contract), 1);
        assert!(
            gen_page_key_count(&contract) <= page_count as usize,
            "initial layout should have at most one physical page per logical page"
        );

        let before_puts = contract.host.borrow().put_count;
        let before_deletes = contract.host.borrow().del_count;
        let second = json_response(&contract.call_update(
            "exec",
            &["update pages set payload = 'changed' where id = 32;"],
        )?);
        assert_eq!(second["ok"], true);
        let dirty_pages = second["dirty_pages"].as_u64().unwrap() as usize;
        let put_delta = contract.host.borrow().put_count - before_puts;
        assert!(
            put_delta <= dirty_pages + 2,
            "expected dirty pages plus manifest plus metadata, got {put_delta} puts for {dirty_pages} dirty pages"
        );
        assert!(
            put_delta < page_count as usize,
            "sparse commit should not rewrite every logical page"
        );
        assert!(
            contract.host.borrow().del_count > before_deletes,
            "old page versions should be garbage-collected after metadata promotion"
        );
        assert_eq!(manifest_key_count(&contract), 1);
        assert!(
            gen_page_key_count(&contract) <= page_count as usize,
            "GC should keep physical page versions bounded by logical pages"
        );

        let changed = json_response(
            &contract.call_query("query", &["select payload from pages where id = 32;"])?,
        );
        assert_eq!(changed["rows"][0][0], "changed");
        Ok(())
    }

    #[test]
    fn backup_chunk_streams_sqlite_pages_from_pinned_generation() -> Result<()> {
        let mut contract = Contract::load()?;
        let written = json_response(&contract.call_update(
            "exec",
            &[
                "create table people(first_name text not null, last_name text not null);
insert into people(first_name,last_name) values ('Ada','Lovelace'),('Grace','Hopper');",
            ],
        )?);
        assert_eq!(written["ok"], true);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        let generation = storage["generation"].as_u64().unwrap();
        let page_count = storage["page_count"].as_u64().unwrap();
        let file_bytes = storage["file_bytes"].as_u64().unwrap();
        assert!(page_count > 0);
        assert!(file_bytes > 0);

        let mut image = Vec::new();
        let mut start_page = 1u64;
        while start_page <= page_count {
            let chunk_pages = (page_count - start_page + 1).min(8);
            let chunk = json_response(&contract.call_query(
                "backup_chunk",
                &[
                    &generation.to_string(),
                    &start_page.to_string(),
                    &chunk_pages.to_string(),
                ],
            )?);
            assert_eq!(chunk["ok"], true);
            assert_eq!(chunk["generation"], generation);
            assert_eq!(chunk["page_count"], page_count);
            assert_eq!(chunk["file_bytes"], file_bytes);
            assert_eq!(chunk["start_page"], start_page);
            assert_eq!(chunk["chunk_pages"], chunk_pages);
            let bytes = general_purpose::STANDARD.decode(chunk["data_b64"].as_str().unwrap())?;
            assert_eq!(bytes.len(), chunk_pages as usize * 4096);
            image.extend_from_slice(&bytes);
            start_page += chunk_pages;
        }
        image.truncate(file_bytes as usize);
        assert_eq!(&image[..16], b"SQLite format 3\0");

        let backup_path = std::env::temp_dir().join(format!(
            "octra-sqlite-backup-test-{}-{generation}.sqlite",
            std::process::id()
        ));
        fs::write(&backup_path, &image)?;
        let integrity = ProcessCommand::new("sqlite3")
            .arg(&backup_path)
            .arg("pragma integrity_check;")
            .output()?;
        if !integrity.status.success() {
            bail!(
                "sqlite3 integrity_check failed: {}",
                String::from_utf8_lossy(&integrity.stderr)
            );
        }
        assert_eq!(String::from_utf8_lossy(&integrity.stdout).trim(), "ok");
        let readback = ProcessCommand::new("sqlite3")
            .arg(&backup_path)
            .arg("select first_name || ' ' || last_name from people order by first_name;")
            .output()?;
        let _ = fs::remove_file(&backup_path);
        if !readback.status.success() {
            bail!(
                "sqlite3 readback failed: {}",
                String::from_utf8_lossy(&readback.stderr)
            );
        }
        assert_eq!(
            String::from_utf8_lossy(&readback.stdout),
            "Ada Lovelace\nGrace Hopper\n"
        );

        let stale = json_response(&contract.call_query(
            "backup_chunk",
            &["999", "1", &page_count.min(8).to_string()],
        )?);
        assert_eq!(stale["ok"], false);
        assert_eq!(stale["error"], "backup_generation_changed");
        Ok(())
    }

    #[test]
    fn replaying_same_history_is_byte_identical() -> Result<()> {
        fn run_history() -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
            let mut contract = Contract::load()?;
            contract.call_update(
                "exec",
                &["create table people(first_name text not null, last_name text not null);
insert into people(first_name,last_name) values ('Ada','Byron'),('Katherine','Johnson'),('Margaret','Hamilton');"],
            )?;
            let kv = contract.host.borrow().kv.clone();
            Ok(kv)
        }
        assert_eq!(run_history()?, run_history()?);
        Ok(())
    }

    #[test]
    fn malformed_and_oversized_frames_fail_closed() -> Result<()> {
        let mut contract = Contract::load()?;
        let (rc, response) = contract.call_raw_query(b"nope")?;
        assert_eq!(rc, 10);
        assert!(response.is_none());

        let (rc, response) = contract.call_raw_query(b"BAD!!\0\0\0\0")?;
        assert_eq!(rc, 11);
        assert!(response.is_none());

        let oversized = "x".repeat(8192);
        let frame = call_frame("query", &[&oversized]);
        let (rc, response) = contract.call_raw_query(&frame)?;
        assert_eq!(rc, 17);
        assert!(response.is_none());
        Ok(())
    }

    #[test]
    fn failed_kv_flush_does_not_promote_metadata() -> Result<()> {
        let mut contract = Contract::load()?;
        contract.host.borrow_mut().fail_put_after = Some(0);
        let failed = json_response(&contract.call_update(
            "exec",
            &["create table people(first_name text not null, last_name text not null);"],
        )?);
        assert_eq!(failed["ok"], false);
        assert!(!contract
            .host
            .borrow()
            .kv
            .contains_key(b"octra.sqlite.vfs.v1.meta".as_slice()));
        Ok(())
    }

    #[test]
    fn failed_kv_flush_does_not_consume_owner_sequence() -> Result<()> {
        let (owner, owner_pubkey, db_id_bytes) = owner_fixture();
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;
        let pubkey = hex::encode(owner_pubkey);
        let sql = "create table people(first_name text not null);";
        let sig = sign_osw1(&owner, &db_id_bytes, 1, "exec", sql);

        contract.host.borrow_mut().fail_put_after = Some(0);
        let failed = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(failed["ok"], false);
        assert!(!contract
            .host
            .borrow()
            .kv
            .contains_key(b"octra.sqlite.vfs.v1.meta".as_slice()));

        contract.host.borrow_mut().fail_put_after = None;
        let retry = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(retry["ok"], true);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 1);
        Ok(())
    }

    #[test]
    fn failed_manifest_write_does_not_promote_pages_or_sequence() -> Result<()> {
        let (owner, owner_pubkey, db_id_bytes) = owner_fixture();
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;
        let pubkey = hex::encode(owner_pubkey);
        let sql = "create table people(first_name text not null);";
        let sig = sign_osw1(&owner, &db_id_bytes, 1, "exec", sql);

        contract.host.borrow_mut().fail_put_key_contains = Some(".manifest".to_string());
        let failed = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(failed["ok"], false);
        assert!(!contract
            .host
            .borrow()
            .kv
            .contains_key(b"octra.sqlite.vfs.v1.meta".as_slice()));
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 0);

        contract.host.borrow_mut().fail_put_key_contains = None;
        let retry = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(retry["ok"], true);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 1);
        Ok(())
    }

    #[test]
    fn failed_metadata_write_does_not_promote_manifest_or_sequence() -> Result<()> {
        let (owner, owner_pubkey, db_id_bytes) = owner_fixture();
        let mut contract = Contract::load_patched(&owner_pubkey, &db_id_bytes)?;
        let pubkey = hex::encode(owner_pubkey);
        let sql = "create table people(first_name text not null);";
        let sig = sign_osw1(&owner, &db_id_bytes, 1, "exec", sql);

        contract.host.borrow_mut().fail_put_key_contains =
            Some("octra.sqlite.vfs.v1.meta".to_string());
        let failed = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(failed["ok"], false);
        assert!(manifest_key_count(&contract) >= 1);
        assert!(!contract
            .host
            .borrow()
            .kv
            .contains_key(b"octra.sqlite.vfs.v1.meta".as_slice()));
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 0);

        contract.host.borrow_mut().fail_put_key_contains = None;
        let retry = json_response(&contract.call_update("exec", &[sql, &pubkey, "1", &sig])?);
        assert_eq!(retry["ok"], true);
        let storage = json_response(&contract.call_query("storage_info", &[])?);
        assert_eq!(storage["owner_sequence"], 1);
        Ok(())
    }

    #[test]
    fn deterministic_now_and_readonly_enforcement() -> Result<()> {
        let mut contract = Contract::load()?;
        contract.call_update("exec", &["create table people(first_name text not null);"])?;
        let now = contract.call_query("query_typed", &["select datetime('now') as now;"])?;
        assert!(now.starts_with("OSR1:"));
        let missing =
            json_response(&contract.call_query("query_typed", &["select * from companion;"])?);
        assert_eq!(missing["ok"], false);
        assert_eq!(missing["error"], "sqlite_prepare_failed");
        assert!(missing["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("no such table: companion")));
        let trailing_line_comment =
            contract.call_query("query_typed", &["select 1 as one; -- trailing comment"])?;
        assert!(trailing_line_comment.starts_with("OSR1:"));
        let trailing_block_comment =
            contract.call_query("query_typed", &["select /* ; */ 1 as one; /* trailing */"])?;
        assert!(trailing_block_comment.starts_with("OSR1:"));
        let second_statement =
            json_response(&contract.call_query("query_typed", &["select 1 as one; select 2;"])?);
        assert_eq!(second_statement["ok"], false);
        assert_eq!(second_statement["error"], "sqlite_single_query_required");
        let second_write_statement = json_response(&contract.call_query(
            "query_typed",
            &["select 1 as one; insert into people(first_name) values ('Mallory');"],
        )?);
        assert_eq!(second_write_statement["ok"], false);
        assert_eq!(
            second_write_statement["error"],
            "sqlite_single_query_required"
        );
        let malformed_tail =
            json_response(&contract.call_query("query_typed", &["select 1 as one; slect 2;"])?);
        assert_eq!(malformed_tail["ok"], false);
        assert_eq!(malformed_tail["error"], "sqlite_tail_prepare_failed");
        let denied = json_response(
            &contract.call_query("query", &["insert into people(first_name) values ('Ada');"])?,
        );
        assert_eq!(denied["ok"], false);
        Ok(())
    }
}

#[cfg(not(feature = "wasm-behavior"))]
#[test]
fn wasm_behavior_harness_is_available_behind_feature() {
    eprintln!(
        "run with --features wasm-behavior after building circle/wasm/octra_sqlite_circle.wasm"
    );
}
