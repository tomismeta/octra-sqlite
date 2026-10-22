typedef unsigned char u8;
typedef unsigned int u32;

__attribute__((import_module("octra"), import_name("host_response_reset")))
extern int host_response_reset(void);
__attribute__((import_module("octra"), import_name("host_response_write")))
extern int host_response_write(const u8 *ptr, int len);
__attribute__((import_module("octra"), import_name("host_response_finish")))
extern int host_response_finish(int status_code);
__attribute__((import_module("octra"), import_name("host_caller_len")))
extern int host_caller_len(void);
__attribute__((import_module("octra"), import_name("host_caller_read")))
extern int host_caller_read(u8 *out_ptr, int out_cap);
__attribute__((import_module("octra"), import_name("host_self_len")))
extern int host_self_len(void);
__attribute__((import_module("octra"), import_name("host_self_read")))
extern int host_self_read(u8 *out_ptr, int out_cap);
__attribute__((import_module("octra"), import_name("host_kv_put")))
extern int host_kv_put(const u8 *key_ptr, int key_len, const u8 *value_ptr, int value_len);

__attribute__((used)) static u8 owner_pubkey[32] = "OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
__attribute__((used)) static u8 database_id[32] = "OSQL_DATABASE_ID_V1_PLACEHOLDER0";
static const u8 owner_address[] = "octCpJ1SJNi7NBNEjo9DnMfhy4fH3HGDrXN7JL1UhoGYgCB";
static const u8 proof_key[] = "octra.native-caller.probe";
static u8 heap[65536];
static u32 heap_pos;
static u8 caller[64];
static u8 self_addr[64];
static u8 json[256];
static u8 frame[266];
static u32 json_len;

static void put_be32(u8 *out, u32 value) {
  out[0] = (u8)(value >> 24); out[1] = (u8)(value >> 16);
  out[2] = (u8)(value >> 8); out[3] = (u8)value;
}

static void append(const u8 *value, u32 len) {
  for (u32 i = 0; i < len && json_len < sizeof(json); ++i) json[json_len++] = value[i];
}

static int equal(const u8 *a, const u8 *b, u32 len) {
  for (u32 i = 0; i < len; ++i) if (a[i] != b[i]) return 0;
  return 1;
}

static int respond_raw(const u8 *value, u32 len, int status) {
  host_response_reset();
  if (host_response_write(value, (int)len) < 0) return 40;
  if (host_response_finish(status) < 0) return 41;
  return status;
}

static int respond_identity(int status) {
  int caller_len = host_caller_len();
  int self_len = host_self_len();
  if (caller_len < 0 || caller_len > 63 || self_len < 0 || self_len > 63) return 2;
  if (host_caller_read(caller, caller_len) != caller_len) return 3;
  if (host_self_read(self_addr, self_len) != self_len) return 4;
  int authorized = caller_len == 47 && equal(caller, owner_address, 47);
  static const u8 a[] = "{\"ok\":";
  static const u8 b[] = ",\"caller\":\"";
  static const u8 c[] = "\",\"self\":\"";
  static const u8 d[] = "\",\"authorized\":";
  static const u8 e[] = "}";
  json_len = 0;
  append(a, sizeof(a) - 1); append((const u8 *)(status == 0 ? "true" : "false"), status == 0 ? 4 : 5);
  append(b, sizeof(b) - 1); append(caller, (u32)caller_len);
  append(c, sizeof(c) - 1); append(self_addr, (u32)self_len);
  append(d, sizeof(d) - 1); append((const u8 *)(authorized ? "true" : "false"), authorized ? 4 : 5);
  append(e, sizeof(e) - 1);
  frame[0] = 'O'; frame[1] = 'C'; frame[2] = 'W'; frame[3] = 'S'; frame[4] = '1'; frame[5] = 4;
  put_be32(frame + 6, json_len);
  for (u32 i = 0; i < json_len; ++i) frame[10 + i] = json[i];
  return respond_raw(frame, 10 + json_len, status);
}

__attribute__((export_name("octra_alloc")))
int octra_alloc(int len) {
  if (len <= 0 || heap_pos + (u32)len > sizeof(heap)) return 0;
  u8 *ptr = heap + heap_pos;
  heap_pos += (u32)len;
  return (int)ptr;
}

__attribute__((export_name("octra_manifest")))
int octra_manifest(int ptr, int len) {
  (void)ptr; (void)len;
  static const u8 manifest[] = "{\"methods\":[{\"name\":\"identity\",\"view\":true},{\"name\":\"write_probe\",\"view\":false}],\"engine\":\"native-caller-probe-v1\",\"storage\":\"circle_key_value\"}";
  return respond_raw(manifest, sizeof(manifest) - 1, 0);
}

__attribute__((export_name("octra_query")))
int octra_query(int ptr, int len) {
  (void)ptr; (void)len;
  return respond_identity(0);
}

__attribute__((export_name("octra_update")))
int octra_update(int ptr, int len) {
  (void)ptr; (void)len;
  int caller_len = host_caller_len();
  if (caller_len != 47 || host_caller_read(caller, caller_len) != caller_len || !equal(caller, owner_address, 47)) {
    return respond_identity(403);
  }
  if (host_kv_put(proof_key, sizeof(proof_key) - 1, caller, (int)caller_len) < 0) return 5;
  return respond_identity(0);
}
