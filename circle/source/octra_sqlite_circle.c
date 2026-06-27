typedef unsigned int u32;
typedef unsigned long usize;
typedef long long i64;
typedef unsigned long long u64;
typedef unsigned char u8;

#include "sqlite3.h"
#include "tweetnacl.h"

__attribute__((import_module("octra"), import_name("host_response_reset")))
extern int host_response_reset(void);

__attribute__((import_module("octra"), import_name("host_response_write")))
extern int host_response_write(const u8 *ptr, int len);

__attribute__((import_module("octra"), import_name("host_response_finish")))
extern int host_response_finish(int status_code);

__attribute__((import_module("octra"), import_name("host_kv_get_len")))
extern int host_kv_get_len(const u8 *key_ptr, int key_len);

__attribute__((import_module("octra"), import_name("host_kv_get")))
extern int host_kv_get(const u8 *key_ptr, int key_len, u8 *out_ptr, int out_cap);

__attribute__((import_module("octra"), import_name("host_kv_put")))
extern int host_kv_put(const u8 *key_ptr, int key_len, const u8 *value_ptr, int value_len);

__attribute__((import_module("octra"), import_name("host_kv_del")))
extern int host_kv_del(const u8 *key_ptr, int key_len);

__attribute__((import_module("octra"), import_name("host_emit_event")))
extern int host_emit_event(const u8 *topic_ptr, int topic_len, const u8 *data_ptr, int data_len);

#define HEAP_BYTES (20u * 1024u * 1024u)
#define PAGE_SIZE 4096
#define STRINGIFY_VALUE_1(x) #x
#define STRINGIFY_VALUE(x) STRINGIFY_VALUE_1(x)
#define PAGE_SIZE_JSON STRINGIFY_VALUE(PAGE_SIZE)
#define MAX_DIRTY_PAGES 1024
#define MAX_JSON_BYTES 65526
#define MAX_SQL_BYTES 8192
#define MAX_DB_PAGES 8192
#define MAX_DB_PAGES_JSON STRINGIFY_VALUE(MAX_DB_PAGES)
#define MAX_RESULT_ROWS 512
#define TYPED_TEXT_PREFIX "OSR1:"
#define FIXED_JULIAN_DAY 2460486.5
#define FIXED_JULIAN_MS 212586033600000ll
#define ENGINE_ID "sqlite-3.53.2-in-octra-wasm-v1"
#define STORAGE_ID "circle_key_value_page_vfs"
#define OWNER_PUBKEY_PLACEHOLDER_TEXT "OSQL_OWNER_PUBKEY_V1_PLACEHOLDER"
#define DB_ID_PLACEHOLDER_TEXT "OSQL_DATABASE_ID_V1_PLACEHOLDER0"
/* OSW1 project-local owner write intent signature domain. */
#define OWNER_WRITE_INTENT_DOMAIN "octra-sqlite.osw1.v1"
#define OWNER_WRITE_INTENT_DOMAIN_LEN 21u /* Includes the trailing NUL domain separator. */
#define MAX_METHOD_BYTES 16u
#define MAX_OWNER_WRITE_INTENT_BYTES (OWNER_WRITE_INTENT_DOMAIN_LEN + 32u + 8u + 2u + MAX_METHOD_BYTES + 4u + MAX_SQL_BYTES)
#define OCTRA_SQLITE_APP_ERROR 2

typedef char owner_write_intent_domain_len_must_include_nul[(sizeof(OWNER_WRITE_INTENT_DOMAIN) == OWNER_WRITE_INTENT_DOMAIN_LEN) ? 1 : -1];

enum OctraCode {
  OCTRA_ERR_FRAME_TOO_SHORT = 10,
  OCTRA_ERR_BAD_FRAME_MAGIC = 11,
  OCTRA_ERR_FRAME_BOUNDS = 17,
  OCTRA_ERR_BAD_PARAM_TYPE = 20,
  OCTRA_ERR_RESPONSE_WRITE = 40,
  OCTRA_ERR_RESPONSE_FINISH = 41,
  OCTRA_ERR_UNKNOWN_METHOD = 60,
  OCTRA_ERR_PARAM_COUNT = 92,
  OCTRA_ERR_AUTH = 120
};

__attribute__((used))
static u8 configured_owner_pubkey[32] = OWNER_PUBKEY_PLACEHOLDER_TEXT;

__attribute__((used))
static u8 configured_db_id[32] = DB_ID_PLACEHOLDER_TEXT;

static u8 heap[HEAP_BYTES];
static u32 heap_pos = 0;

void *memcpy(void *dst, const void *src, usize n) {
  u8 *d = (u8 *)dst;
  const u8 *s = (const u8 *)src;
  for (usize i = 0; i < n; ++i) d[i] = s[i];
  return dst;
}

void *memmove(void *dst, const void *src, usize n) {
  u8 *d = (u8 *)dst;
  const u8 *s = (const u8 *)src;
  if (d < s) {
    for (usize i = 0; i < n; ++i) d[i] = s[i];
  } else if (d > s) {
    for (usize i = n; i > 0; --i) d[i - 1] = s[i - 1];
  }
  return dst;
}

void *memset(void *dst, int value, usize n) {
  u8 *d = (u8 *)dst;
  for (usize i = 0; i < n; ++i) d[i] = (u8)value;
  return dst;
}

void randombytes(u8 *dst, u64 n) {
  (void)dst;
  (void)n;
  __builtin_trap();
}

int memcmp(const void *lhs, const void *rhs, usize n) {
  const u8 *a = (const u8 *)lhs;
  const u8 *b = (const u8 *)rhs;
  for (usize i = 0; i < n; ++i) {
    if (a[i] != b[i]) return (int)a[i] - (int)b[i];
  }
  return 0;
}

void *memchr(const void *s, int c, usize n) {
  const u8 *p = (const u8 *)s;
  for (usize i = 0; i < n; ++i) {
    if (p[i] == (u8)c) return (void *)(p + i);
  }
  return (void *)0;
}

usize strlen(const char *s) {
  usize n = 0;
  while (s[n]) ++n;
  return n;
}

int strcmp(const char *a, const char *b) {
  while (*a && *a == *b) {
    ++a;
    ++b;
  }
  return (int)(u8)*a - (int)(u8)*b;
}

int strncmp(const char *a, const char *b, usize n) {
  for (usize i = 0; i < n; ++i) {
    if (a[i] != b[i] || !a[i] || !b[i]) return (int)(u8)a[i] - (int)(u8)b[i];
  }
  return 0;
}

char *strchr(const char *s, int c) {
  while (*s) {
    if (*s == (char)c) return (char *)s;
    ++s;
  }
  return c == 0 ? (char *)s : (char *)0;
}

char *strrchr(const char *s, int c) {
  char *last = (char *)0;
  do {
    if (*s == (char)c) last = (char *)s;
  } while (*s++);
  return last;
}

static int is_space(char ch) {
  return ch == ' ' || ch == '\n' || ch == '\r' || ch == '\t' || ch == '\f';
}

static const char *skip_sql_tail(const char *tail) {
  if (!tail) return tail;
  for (;;) {
    while (is_space(*tail)) ++tail;
    if (tail[0] == '-' && tail[1] == '-') {
      tail += 2;
      while (*tail && *tail != '\n' && *tail != '\r') ++tail;
      continue;
    }
    if (tail[0] == '/' && tail[1] == '*') {
      tail += 2;
      while (tail[0] && !(tail[0] == '*' && tail[1] == '/')) ++tail;
      if (tail[0]) tail += 2;
      continue;
    }
    return tail;
  }
}

usize strspn(const char *s, const char *accept) {
  usize n = 0;
  for (; s[n]; ++n) {
    int ok = 0;
    for (usize j = 0; accept[j]; ++j) {
      if (s[n] == accept[j]) {
        ok = 1;
        break;
      }
    }
    if (!ok) break;
  }
  return n;
}

usize strcspn(const char *s, const char *reject) {
  usize n = 0;
  for (; s[n]; ++n) {
    for (usize j = 0; reject[j]; ++j) {
      if (s[n] == reject[j]) return n;
    }
  }
  return n;
}

void *malloc(usize n) {
  if (n == 0) n = 1;
  usize aligned = ((usize)heap_pos + 7u) & ~(usize)7u;
  if (aligned > sizeof(heap) || n > (usize)sizeof(heap) - aligned || n + 8u > (usize)sizeof(heap) - aligned) {
    return (void *)0;
  }
  usize total = n + 8u;
  *((u32 *)(heap + aligned)) = (u32)n;
  heap_pos = (u32)(aligned + total);
  return heap + aligned + 8u;
}

void free(void *ptr) {
  (void)ptr;
}

void *calloc(usize count, usize size) {
  if (count != 0 && size > ((usize)~(usize)0) / count) return (void *)0;
  usize n = count * size;
  void *p = malloc(n);
  if (p) memset(p, 0, n);
  return p;
}

void *realloc(void *ptr, usize n) {
  if (!ptr) return malloc(n);
  u32 old = *((u32 *)((u8 *)ptr - 8u));
  void *next = malloc(n);
  if (next) memcpy(next, ptr, old < n ? old : n);
  return next;
}

double fabs(double x) {
  return x < 0 ? -x : x;
}

double strtod(const char *s, char **end) {
  const char *start = s;
  i64 sign = 1;
  i64 whole = 0;
  double frac = 0.0;
  double scale = 1.0;
  int saw_digit = 0;
  while (is_space(*s)) ++s;
  if (*s == '-' || *s == '+') {
    if (*s == '-') sign = -1;
    ++s;
  }
  while (*s >= '0' && *s <= '9') {
    saw_digit = 1;
    whole = whole * 10 + (*s - '0');
    ++s;
  }
  if (*s == '.') {
    ++s;
    while (*s >= '0' && *s <= '9') {
      saw_digit = 1;
      frac = frac * 10.0 + (double)(*s - '0');
      scale *= 10.0;
      ++s;
    }
  }
  double value = (double)sign * ((double)whole + frac / scale);
  if (saw_digit && (*s == 'e' || *s == 'E')) {
    const char *exp_start = s;
    i64 exp_sign = 1;
    int exp = 0;
    int saw_exp_digit = 0;
    ++s;
    if (*s == '-' || *s == '+') {
      if (*s == '-') exp_sign = -1;
      ++s;
    }
    while (*s >= '0' && *s <= '9') {
      saw_exp_digit = 1;
      if (exp < 308) exp = exp * 10 + (*s - '0');
      ++s;
    }
    if (saw_exp_digit) {
      while (exp > 0) {
        value = exp_sign > 0 ? value * 10.0 : value / 10.0;
        --exp;
      }
    } else {
      s = exp_start;
    }
  }
  if (!saw_digit) {
    if (end) *end = (char *)start;
    return 0.0;
  }
  if (end) *end = (char *)s;
  return value;
}

__attribute__((export_name("octra_alloc")))
int octra_alloc(int len) {
  if (len <= 0) return 0;
  return (int)malloc((usize)len);
}

static u32 be16(const u8 *p) {
  return ((u32)p[0] << 8) | (u32)p[1];
}

static u32 be32(const u8 *p) {
  return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | (u32)p[3];
}

static u64 be64(const u8 *p) {
  u64 hi = (u64)be32(p);
  u64 lo = (u64)be32(p + 4);
  return (hi << 32) | lo;
}

static void put_be16(u8 *p, u32 n) {
  p[0] = (u8)(n >> 8);
  p[1] = (u8)n;
}

static void put_be32(u8 *p, u32 n) {
  p[0] = (u8)(n >> 24);
  p[1] = (u8)(n >> 16);
  p[2] = (u8)(n >> 8);
  p[3] = (u8)n;
}

static void put_be64(u8 *p, u64 n) {
  put_be32(p, (u32)(n >> 32));
  put_be32(p + 4, (u32)n);
}

static int streq_bytes(const u8 *a, u32 alen, const char *b) {
  u32 i = 0;
  for (; i < alen; ++i) {
    if (b[i] == 0 || a[i] != (u8)b[i]) return 0;
  }
  return b[i] == 0;
}

static int streq_cstr(const char *a, const char *b) {
  while (*a && *b) {
    if (*a++ != *b++) return 0;
  }
  return *a == 0 && *b == 0;
}

static u8 out[MAX_JSON_BYTES];
static u32 out_len;
static int out_overflow;
static int auth_policy_error;
static int auth_policy_status_code;
static int row_count;

static void reset_output(void) {
  out_len = 0;
  out_overflow = 0;
  auth_policy_error = 0;
  auth_policy_status_code = 0;
  row_count = 0;
}

static void append_byte(u8 ch) {
  if (out_len < sizeof(out)) {
    out[out_len++] = ch;
  } else {
    out_overflow = 1;
  }
}

static void append_cstr(const char *s) {
  while (*s) append_byte((u8)*s++);
}

static void append_bytes(const u8 *s, u32 n) {
  if (n == 0) return;
  if (!s || n > sizeof(out) - out_len) {
    out_overflow = 1;
    return;
  }
  memcpy(out + out_len, s, n);
  out_len += n;
}

static void append_json_string_bytes(const u8 *s, int n) {
  append_byte('"');
  if (s && n > 0) {
    for (int i = 0; i < n; ++i) {
      u8 ch = s[i];
      if (ch == '"' || ch == '\\') {
        append_byte('\\');
        append_byte(ch);
      } else if (ch == '\n') {
        append_cstr("\\n");
      } else if (ch == '\r') {
        append_cstr("\\r");
      } else if (ch == '\t') {
        append_cstr("\\t");
      } else if (ch < 0x20) {
        const char *hex = "0123456789abcdef";
        append_cstr("\\u00");
        append_byte((u8)hex[ch >> 4]);
        append_byte((u8)hex[ch & 15]);
      } else {
        append_byte(ch);
      }
    }
  }
  append_byte('"');
}

static void append_json_string(const char *s) {
  append_json_string_bytes((const u8 *)s, s ? (int)strlen(s) : 0);
}

static void append_i64(i64 value) {
  char buf[32];
  u32 n = 0;
  u64 x;
  if (value < 0) {
    append_byte('-');
    x = (u64)(-(value + 1)) + 1u;
  } else {
    x = (u64)value;
  }
  do {
    buf[n++] = (char)('0' + (x % 10ull));
    x /= 10ull;
  } while (x);
  while (n) append_byte((u8)buf[--n]);
}

static void append_hex_byte(u8 value) {
  static const char hex[] = "0123456789abcdef";
  append_byte((u8)hex[value >> 4]);
  append_byte((u8)hex[value & 15u]);
}

static void append_hex_bytes(const u8 *value, u32 len) {
  for (u32 i = 0; i < len; ++i) append_hex_byte(value[i]);
}

static void append_be32_value(u32 value) {
  u8 buf[4];
  put_be32(buf, value);
  append_bytes(buf, sizeof(buf));
}

static void append_be64_value(u64 value) {
  u8 buf[8];
  put_be64(buf, value);
  append_bytes(buf, sizeof(buf));
}

static void patch_be32(u32 offset, u32 value) {
  if (offset + 4u > out_len) {
    out_overflow = 1;
    return;
  }
  put_be32(out + offset, value);
}

static void append_json_error(const char *error, const char *detail) {
  append_cstr("{\"ok\":false,\"error\":");
  append_json_string(error);
  append_cstr(",\"detail\":");
  append_json_string(detail);
  append_byte('}');
}

static void emit_auth_event(const char *error, const char *detail) {
  static u8 payload[192];
  const char *prefix =
      (streq_cstr(error, "auth_required") ||
       streq_cstr(error, "auth_bad_encoding") ||
       streq_cstr(error, "auth_bad_signature"))
          ? "auth_not_authenticated:"
          : "auth_not_authorized:";
  u32 w = 0;
  while (*prefix && w < sizeof(payload)) payload[w++] = (u8)*prefix++;
  while (*error && w < sizeof(payload)) payload[w++] = (u8)*error++;
  if (detail && *detail && w < sizeof(payload)) payload[w++] = ':';
  while (detail && *detail && w < sizeof(payload)) payload[w++] = (u8)*detail++;
  host_emit_event((const u8 *)"octra.sqlite.auth", 17, payload, (int)w);
}

static void emit_sql_error_event(const char *error, const char *detail) {
  static u8 payload[256];
  u32 w = 0;
  while (error && *error && w < sizeof(payload)) payload[w++] = (u8)*error++;
  if (detail && *detail && w < sizeof(payload)) payload[w++] = ':';
  while (detail && *detail && w < sizeof(payload)) payload[w++] = (u8)*detail++;
  host_emit_event((const u8 *)"octra.sqlite.error", 18, payload, (int)w);
}

static int auth_status_code(const char *error) {
  if (streq_cstr(error, "auth_denied")) return 403;
  if (streq_cstr(error, "auth_required") ||
      streq_cstr(error, "auth_bad_signature")) return 401;
  if (streq_cstr(error, "auth_replay")) return 409;
  if (streq_cstr(error, "auth_message_too_large")) return 413;
  if (streq_cstr(error, "auth_bad_encoding") ||
      streq_cstr(error, "auth_bad_sequence")) return 400;
  return 500;
}

static void append_auth_error(const char *error, const char *detail) {
  auth_policy_error = 1;
  auth_policy_status_code = auth_status_code(error);
  emit_auth_event(error, detail);
  append_cstr("{\"ok\":false,\"error\":");
  append_json_string(error);
  append_cstr(",\"detail\":");
  append_json_string(detail);
  append_cstr(",\"auth\":\"osw1\",\"authorized\":false}");
}

static int set_json_error(const char *error, const char *detail) {
  reset_output();
  append_json_error(error, detail);
  return 1;
}

static void append_result_envelope_open(void) {
  append_cstr("{\"ok\":true,\"engine\":\"");
  append_cstr(ENGINE_ID);
  append_cstr("\",\"storage\":\"");
  append_cstr(STORAGE_ID);
  append_cstr("\",\"page_size\":");
  append_cstr(PAGE_SIZE_JSON);
}

static int respond_raw(const u8 *bytes, u32 len, int status_code) {
  host_response_reset();
  if (host_response_write(bytes, (int)len) < 0) return OCTRA_ERR_RESPONSE_WRITE;
  if (host_response_finish(status_code) < 0) return OCTRA_ERR_RESPONSE_FINISH;
  return status_code == 0 ? 0 : status_code;
}

static int respond_string_bytes(const u8 *s, u32 payload_len, int status_code) {
  static u8 frame[65536];
  static const char too_large[] = "{\"ok\":false,\"error\":\"response_too_large\",\"detail\":\"response frame capacity exceeded\"}";
  if (payload_len > sizeof(frame) - 10u) {
    s = (const u8 *)too_large;
    payload_len = (u32)sizeof(too_large) - 1u;
    status_code = 1;
  }
  frame[0] = 'O'; frame[1] = 'C'; frame[2] = 'W'; frame[3] = 'S'; frame[4] = '1';
  frame[5] = 4;
  put_be32(frame + 6, payload_len);
  memcpy(frame + 10, s, payload_len);
  return respond_raw(frame, 10 + payload_len, status_code);
}

static int respond_cstr(const char *s, int status_code) {
  return respond_string_bytes((const u8 *)s, (u32)strlen(s), status_code);
}

static int respond_json_result(int status_code) {
  if (status_code == 0 && out_overflow) {
    set_json_error("response_too_large", "query result exceeded contract response buffer");
    status_code = 1;
  }
  return respond_string_bytes(out, out_len, status_code);
}

static int respond_typed_result(int status_code) {
  static u8 text[MAX_JSON_BYTES];
  static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  if (status_code != 0) return respond_json_result(status_code);
  if (out_overflow) {
    set_json_error("response_too_large", "typed query result exceeded contract response buffer");
    return respond_json_result(1);
  }
  u32 encoded_len = (u32)sizeof(TYPED_TEXT_PREFIX) - 1u + 4u * ((out_len + 2u) / 3u);
  if (encoded_len > sizeof(text)) {
    set_json_error("response_too_large", "typed query result exceeded contract response buffer");
    return respond_json_result(1);
  }
  u32 w = 0;
  for (u32 i = 0; i < (u32)sizeof(TYPED_TEXT_PREFIX) - 1u; ++i) {
    text[w++] = (u8)TYPED_TEXT_PREFIX[i];
  }
  for (u32 i = 0; i < out_len; i += 3u) {
    u32 remain = out_len - i;
    u32 a = out[i];
    u32 b = remain > 1u ? out[i + 1u] : 0u;
    u32 c = remain > 2u ? out[i + 2u] : 0u;
    u32 triple = (a << 16) | (b << 8) | c;
    text[w++] = (u8)alphabet[(triple >> 18) & 63u];
    text[w++] = (u8)alphabet[(triple >> 12) & 63u];
    text[w++] = remain > 1u ? (u8)alphabet[(triple >> 6) & 63u] : (u8)'=';
    text[w++] = remain > 2u ? (u8)alphabet[triple & 63u] : (u8)'=';
  }
  return respond_string_bytes(text, w, 0);
}

/* The manifest ABI is raw JSON; method results use the OCWS1 response frame. */
static int respond_manifest(void) {
  static const char json[] =
      "{\"methods\":["
      "{\"name\":\"health\",\"view\":true},"
      "{\"name\":\"storage_info\",\"view\":true},"
      "{\"name\":\"schema\",\"view\":true},"
      "{\"name\":\"schema_typed\",\"view\":true},"
      "{\"name\":\"query\",\"view\":true},"
      "{\"name\":\"query_typed\",\"view\":true},"
      "{\"name\":\"auth_info\",\"view\":true},"
      "{\"name\":\"exec\",\"view\":false},"
      "{\"name\":\"exec_trace\",\"view\":false},"
      "{\"name\":\"reset\",\"view\":false}"
      "],\"engine\":\"" ENGINE_ID "\","
      "\"storage\":\"" STORAGE_ID "\","
      "\"page_size\":" PAGE_SIZE_JSON "}";
  return respond_raw((const u8 *)json, sizeof(json) - 1, 0);
}

static const char meta_key[] = "octra.sqlite.vfs.v1.meta";
static const char page_key_prefix[] = "octra.sqlite.vfs.v1.page.";
static const char gen_page_key_prefix[] = "octra.sqlite.vfs.v1.gen.";
static const u8 meta_magic[8] = {'O','S','Q','L','V','F','S','1'};
static const u8 meta_magic_v2[8] = {'O','S','Q','L','V','F','S','2'};
static const u8 meta_magic_v3[8] = {'O','S','Q','L','V','F','S','3'};
static const u8 meta_magic_v4[8] = {'O','S','Q','L','V','F','S','4'};

enum MetaVersion {
  META_NONE = 0,
  META_DIRECT_PAGES_V1 = 1,
  META_FULL_GENERATION_V2 = 2,
  META_MANIFEST_GENERATION_V3 = 3,
  META_MANIFEST_WITH_AUTH_V4 = 4
};

typedef struct DirtyPage DirtyPage;
struct DirtyPage {
  u32 page_no;
  u8 data[PAGE_SIZE];
};

typedef struct OctraFile OctraFile;
struct OctraFile {
  sqlite3_file base;
  int is_main;
  int readonly;
  sqlite3_int64 mem_size;
  sqlite3_int64 mem_cap;
  u8 *mem;
};

static DirtyPage dirty_pages[MAX_DIRTY_PAGES];
static int dirty_count;
static sqlite3_int64 main_file_size;
static sqlite3_int64 committed_file_size;
static u64 current_generation;
static int meta_version;
static int meta_loaded;
static int meta_exists;
static int manifest_loaded;
static sqlite3_int64 manifest_page_count;
static int write_failed;
static u64 current_owner_sequence;
static u64 pending_owner_sequence;
static int pending_owner_sequence_active;
static u64 page_generations[MAX_DB_PAGES];
static u64 next_page_generations[MAX_DB_PAGES];
static u8 manifest_bytes[MAX_DB_PAGES * 8u];

static int append_hex8(char *out_key, int pos, u32 n) {
  const char *hex = "0123456789abcdef";
  for (int i = 7; i >= 0; --i) {
    out_key[pos + i] = hex[n & 15u];
    n >>= 4;
  }
  return pos + 8;
}

static int append_hex16(char *out_key, int pos, u64 n) {
  const char *hex = "0123456789abcdef";
  for (int i = 15; i >= 0; --i) {
    out_key[pos + i] = hex[n & 15u];
    n >>= 4;
  }
  return pos + 16;
}

static u64 fnv1a64(const char *s) {
  u64 hash = 1469598103934665603ull;
  while (*s) {
    hash ^= (u8)*s++;
    hash *= 1099511628211ull;
  }
  return hash;
}

static int append_hex64(char *out_buf, int pos, u64 n) {
  const char *hex = "0123456789abcdef";
  for (int i = 15; i >= 0; --i) {
    out_buf[pos + i] = hex[n & 15u];
    n >>= 4;
  }
  return pos + 16;
}

static void emit_exec_event(const char *sql, int trace_sql) {
  char payload[32];
  const char prefix[] = "sql_fnv1a64:";
  int pos = 0;
  for (int i = 0; prefix[i]; ++i) payload[pos++] = prefix[i];
  pos = append_hex64(payload, pos, fnv1a64(sql));
  host_emit_event((const u8 *)"octra.sqlite.exec", 17, (const u8 *)payload, pos);
  if (trace_sql) {
    static u8 trace[MAX_SQL_BYTES + 16];
    const char trace_prefix[] = "sql_text:";
    u32 w = 0;
    while (trace_prefix[w]) {
      trace[w] = (u8)trace_prefix[w];
      ++w;
    }
    u32 sql_len = (u32)strlen(sql);
    u32 cap = (u32)sizeof(trace) - w;
    if (sql_len > cap) sql_len = cap;
    memcpy(trace + w, sql, sql_len);
    host_emit_event((const u8 *)"octra.sqlite.sql", 16, trace, w + sql_len);
  }
}

static int make_page_key(u32 page_no, char *out_key) {
  int pos = 0;
  while (page_key_prefix[pos]) {
    out_key[pos] = page_key_prefix[pos];
    ++pos;
  }
  pos = append_hex8(out_key, pos, page_no);
  out_key[pos] = 0;
  return pos;
}

static int make_gen_page_key(u64 generation, u32 page_no, char *out_key) {
  int pos = 0;
  while (gen_page_key_prefix[pos]) {
    out_key[pos] = gen_page_key_prefix[pos];
    ++pos;
  }
  pos = append_hex16(out_key, pos, generation);
  out_key[pos++] = '.';
  out_key[pos++] = 'p';
  out_key[pos++] = 'a';
  out_key[pos++] = 'g';
  out_key[pos++] = 'e';
  out_key[pos++] = '.';
  pos = append_hex8(out_key, pos, page_no);
  out_key[pos] = 0;
  return pos;
}

static int make_manifest_key(u64 generation, char *out_key) {
  int pos = 0;
  while (gen_page_key_prefix[pos]) {
    out_key[pos] = gen_page_key_prefix[pos];
    ++pos;
  }
  pos = append_hex16(out_key, pos, generation);
  out_key[pos++] = '.';
  out_key[pos++] = 'm';
  out_key[pos++] = 'a';
  out_key[pos++] = 'n';
  out_key[pos++] = 'i';
  out_key[pos++] = 'f';
  out_key[pos++] = 'e';
  out_key[pos++] = 's';
  out_key[pos++] = 't';
  out_key[pos] = 0;
  return pos;
}

static sqlite3_int64 page_count_for_size(sqlite3_int64 size) {
  return (size + PAGE_SIZE - 1) / PAGE_SIZE;
}

static int load_meta(void) {
  if (meta_loaded) return SQLITE_OK;
  meta_loaded = 1;
  meta_exists = 0;
  main_file_size = 0;
  committed_file_size = 0;
  current_generation = 0;
  current_owner_sequence = 0;
  meta_version = META_NONE;

  int len = host_kv_get_len((const u8 *)meta_key, (int)sizeof(meta_key) - 1);
  if (len == -1) return SQLITE_OK;
  if (len < 20) return SQLITE_CORRUPT;

  u8 meta[40];
  if (len > (int)sizeof(meta)) return SQLITE_CORRUPT;
  int got = host_kv_get((const u8 *)meta_key, (int)sizeof(meta_key) - 1, meta, len);
  if (got != len) return SQLITE_IOERR_READ;
  if (memcmp(meta, meta_magic_v4, 8) == 0) {
    if (len != 36) return SQLITE_CORRUPT;
    if (be32(meta + 8) != PAGE_SIZE) return SQLITE_CORRUPT;
    main_file_size = (sqlite3_int64)be64(meta + 12);
    current_generation = be64(meta + 20);
    current_owner_sequence = be64(meta + 28);
    if (current_generation == 0) return SQLITE_CORRUPT;
    meta_version = META_MANIFEST_WITH_AUTH_V4;
  } else if (memcmp(meta, meta_magic_v3, 8) == 0) {
    if (len != 28) return SQLITE_CORRUPT;
    if (be32(meta + 8) != PAGE_SIZE) return SQLITE_CORRUPT;
    main_file_size = (sqlite3_int64)be64(meta + 12);
    current_generation = be64(meta + 20);
    if (current_generation == 0) return SQLITE_CORRUPT;
    meta_version = META_MANIFEST_GENERATION_V3;
  } else if (memcmp(meta, meta_magic_v2, 8) == 0) {
    if (len != 28) return SQLITE_CORRUPT;
    if (be32(meta + 8) != PAGE_SIZE) return SQLITE_CORRUPT;
    main_file_size = (sqlite3_int64)be64(meta + 12);
    current_generation = be64(meta + 20);
    if (current_generation == 0) return SQLITE_CORRUPT;
    meta_version = META_FULL_GENERATION_V2;
  } else if (memcmp(meta, meta_magic, 8) == 0) {
    if (len != 20) return SQLITE_CORRUPT;
    if (be32(meta + 8) != PAGE_SIZE) return SQLITE_CORRUPT;
    main_file_size = (sqlite3_int64)be64(meta + 12);
    current_generation = 0;
    meta_version = META_DIRECT_PAGES_V1;
  } else {
    return SQLITE_CORRUPT;
  }
  committed_file_size = main_file_size;
  meta_exists = 1;
  return SQLITE_OK;
}

static int persist_meta(u64 generation) {
  u8 meta[36];
  u64 owner_sequence = pending_owner_sequence_active ? pending_owner_sequence : current_owner_sequence;
  memcpy(meta, meta_magic_v4, 8);
  put_be32(meta + 8, PAGE_SIZE);
  put_be64(meta + 12, (u64)main_file_size);
  put_be64(meta + 20, generation);
  put_be64(meta + 28, owner_sequence);
  return host_kv_put((const u8 *)meta_key, (int)sizeof(meta_key) - 1, meta, sizeof(meta));
}

static DirtyPage *find_dirty_page(u32 page_no) {
  for (int i = 0; i < dirty_count; ++i) {
    if (dirty_pages[i].page_no == page_no) return &dirty_pages[i];
  }
  return (DirtyPage *)0;
}

static int read_page_version_from_kv(u32 page_no, u64 generation, u8 *page) {
  char key[64];
  int key_len = generation ? make_gen_page_key(generation, page_no, key) : make_page_key(page_no, key);
  memset(page, 0, PAGE_SIZE);
  int len = host_kv_get_len((const u8 *)key, key_len);
  if (len == -1) return generation ? SQLITE_CORRUPT : SQLITE_OK;
  if (len < 0) return SQLITE_IOERR_READ;
  if (len != PAGE_SIZE) return SQLITE_CORRUPT;
  int got = host_kv_get((const u8 *)key, key_len, page, PAGE_SIZE);
  if (got != PAGE_SIZE) return SQLITE_IOERR_READ;
  return SQLITE_OK;
}

static int load_manifest(void) {
  if (manifest_loaded) return SQLITE_OK;
  manifest_page_count = page_count_for_size(committed_file_size);
  if (manifest_page_count > MAX_DB_PAGES) return SQLITE_FULL;
  for (int i = 0; i < MAX_DB_PAGES; ++i) page_generations[i] = 0;
  if (!meta_exists || current_generation == 0) {
    manifest_loaded = 1;
    return SQLITE_OK;
  }

  if (meta_version == META_FULL_GENERATION_V2) {
    for (sqlite3_int64 i = 0; i < manifest_page_count; ++i) {
      page_generations[i] = current_generation;
    }
    manifest_loaded = 1;
    return SQLITE_OK;
  }

  if (meta_version != META_MANIFEST_GENERATION_V3 && meta_version != META_MANIFEST_WITH_AUTH_V4) {
    manifest_loaded = 1;
    return SQLITE_OK;
  }
  if (manifest_page_count == 0) {
    manifest_loaded = 1;
    return SQLITE_OK;
  }

  char key[64];
  int key_len = make_manifest_key(current_generation, key);
  int want = (int)(manifest_page_count * 8);
  int len = host_kv_get_len((const u8 *)key, key_len);
  if (len == -1) return SQLITE_CORRUPT;
  if (len < 0) return SQLITE_IOERR_READ;
  if (len != want) return SQLITE_CORRUPT;
  int got = host_kv_get((const u8 *)key, key_len, manifest_bytes, want);
  if (got != want) return SQLITE_IOERR_READ;
  for (sqlite3_int64 i = 0; i < manifest_page_count; ++i) {
    u64 generation = be64(manifest_bytes + (i * 8));
    if (generation > current_generation) return SQLITE_CORRUPT;
    page_generations[i] = generation;
  }
  manifest_loaded = 1;
  return SQLITE_OK;
}

static int read_page_from_kv(u32 page_no, u8 *page) {
  int rc = load_manifest();
  if (rc != SQLITE_OK) return rc;
  u64 generation = 0;
  if (page_no > 0 && (sqlite3_int64)page_no <= manifest_page_count) {
    generation = page_generations[page_no - 1u];
  }
  return read_page_version_from_kv(page_no, generation, page);
}

static int persist_manifest(u64 generation, sqlite3_int64 page_count, const u64 *generations) {
  if (page_count == 0) return SQLITE_OK;
  if (page_count > MAX_DB_PAGES) return SQLITE_FULL;
  for (sqlite3_int64 i = 0; i < page_count; ++i) {
    put_be64(manifest_bytes + (i * 8), generations[i]);
  }
  char key[64];
  int key_len = make_manifest_key(generation, key);
  int len = (int)(page_count * 8);
  int code = host_kv_put((const u8 *)key, key_len, manifest_bytes, len);
  return code < 0 ? SQLITE_IOERR_WRITE : SQLITE_OK;
}

static void delete_page_version(u32 page_no, u64 generation) {
  char key[64];
  int key_len = generation ? make_gen_page_key(generation, page_no, key) : make_page_key(page_no, key);
  host_kv_del((const u8 *)key, key_len);
}

static void delete_manifest(u64 generation) {
  if (generation == 0) return;
  char key[64];
  int key_len = make_manifest_key(generation, key);
  host_kv_del((const u8 *)key, key_len);
}

static void gc_replaced_pages(sqlite3_int64 old_count, sqlite3_int64 new_count, const u64 *new_generations) {
  for (sqlite3_int64 i = 0; i < old_count; ++i) {
    u64 old_generation = page_generations[i];
    u64 new_generation = i < new_count ? new_generations[i] : 0;
    if (old_generation != new_generation) delete_page_version((u32)i + 1u, old_generation);
  }
  if (meta_version == META_MANIFEST_GENERATION_V3 || meta_version == META_MANIFEST_WITH_AUTH_V4) {
    delete_manifest(current_generation);
  }
}

static void zero_tail_after_file_size(u32 page_no, u8 *page, sqlite3_int64 size) {
  sqlite3_int64 page_start = ((sqlite3_int64)page_no - 1) * PAGE_SIZE;
  sqlite3_int64 page_end = page_start + PAGE_SIZE;
  if (size <= page_start) {
    memset(page, 0, PAGE_SIZE);
  } else if (size < page_end) {
    int keep = (int)(size - page_start);
    if (keep < PAGE_SIZE) memset(page + keep, 0, PAGE_SIZE - keep);
  }
}

static int get_page_for_read(u32 page_no, u8 *page) {
  DirtyPage *dirty = find_dirty_page(page_no);
  if (dirty) {
    memcpy(page, dirty->data, PAGE_SIZE);
    return SQLITE_OK;
  }
  int rc = read_page_from_kv(page_no, page);
  if (rc != SQLITE_OK) return rc;
  zero_tail_after_file_size(page_no, page, main_file_size);
  return SQLITE_OK;
}

static DirtyPage *get_dirty_page(u32 page_no) {
  DirtyPage *dirty = find_dirty_page(page_no);
  if (dirty) return dirty;
  if (dirty_count >= MAX_DIRTY_PAGES) {
    write_failed = SQLITE_FULL;
    return (DirtyPage *)0;
  }
  dirty = &dirty_pages[dirty_count++];
  dirty->page_no = page_no;
  int rc = read_page_from_kv(page_no, dirty->data);
  if (rc != SQLITE_OK) {
    memset(dirty->data, 0, PAGE_SIZE);
    write_failed = rc;
    return (DirtyPage *)0;
  }
  zero_tail_after_file_size(page_no, dirty->data, main_file_size);
  return dirty;
}

static int flush_dirty_pages(void) {
  sqlite3_int64 old_page_count = page_count_for_size(committed_file_size);
  sqlite3_int64 new_page_count = page_count_for_size(main_file_size);
  if (old_page_count > MAX_DB_PAGES || new_page_count > MAX_DB_PAGES) return SQLITE_FULL;
  if (dirty_count == 0 && main_file_size == committed_file_size && !pending_owner_sequence_active) {
    return SQLITE_OK;
  }
  int rc = load_manifest();
  if (rc != SQLITE_OK) return rc;
  u64 next_generation = current_generation + 1u;
  if (next_generation == 0) return SQLITE_FULL;

  for (sqlite3_int64 i = 0; i < new_page_count; ++i) {
    next_page_generations[i] = i < manifest_page_count ? page_generations[i] : 0;
  }

  for (int i = 0; i < dirty_count; ++i) {
    u32 page_no = dirty_pages[i].page_no;
    if (page_no == 0 || (sqlite3_int64)page_no > new_page_count) continue;
    char key[64];
    zero_tail_after_file_size(page_no, dirty_pages[i].data, main_file_size);
    int key_len = make_gen_page_key(next_generation, page_no, key);
    int code = host_kv_put((const u8 *)key, key_len, dirty_pages[i].data, PAGE_SIZE);
    if (code < 0) return SQLITE_IOERR_WRITE;
    next_page_generations[page_no - 1u] = next_generation;
  }

  rc = persist_manifest(next_generation, new_page_count, next_page_generations);
  if (rc != SQLITE_OK) return rc;

  int meta_code = persist_meta(next_generation);
  if (meta_code < 0) return SQLITE_IOERR_WRITE;
  gc_replaced_pages(old_page_count, new_page_count, next_page_generations);
  for (sqlite3_int64 i = 0; i < new_page_count; ++i) {
    page_generations[i] = next_page_generations[i];
  }
  current_generation = next_generation;
  if (pending_owner_sequence_active) {
    current_owner_sequence = pending_owner_sequence;
    pending_owner_sequence = 0;
    pending_owner_sequence_active = 0;
  }
  committed_file_size = main_file_size;
  manifest_page_count = new_page_count;
  manifest_loaded = 1;
  meta_version = META_MANIFEST_WITH_AUTH_V4;
  meta_exists = 1;
  return SQLITE_OK;
}

static int main_read(void *buf, int amount, sqlite3_int64 offset) {
  int short_read = 0;
  u8 *dst = (u8 *)buf;
  memset(dst, 0, (usize)amount);
  if (offset + amount > main_file_size) short_read = 1;

  int copied = 0;
  while (copied < amount) {
    sqlite3_int64 absolute = offset + copied;
    if (absolute >= main_file_size) break;
    u32 page_no = (u32)(absolute / PAGE_SIZE) + 1u;
    int page_off = (int)(absolute % PAGE_SIZE);
    int take = PAGE_SIZE - page_off;
    if (take > amount - copied) take = amount - copied;
    if (absolute + take > main_file_size) take = (int)(main_file_size - absolute);
    u8 page[PAGE_SIZE];
    int rc = get_page_for_read(page_no, page);
    if (rc != SQLITE_OK) return rc;
    memcpy(dst + copied, page + page_off, (usize)take);
    copied += take;
  }
  return short_read ? SQLITE_IOERR_SHORT_READ : SQLITE_OK;
}

static int main_write(const void *buf, int amount, sqlite3_int64 offset) {
  const u8 *src = (const u8 *)buf;
  int copied = 0;
  while (copied < amount) {
    sqlite3_int64 absolute = offset + copied;
    u32 page_no = (u32)(absolute / PAGE_SIZE) + 1u;
    int page_off = (int)(absolute % PAGE_SIZE);
    int take = PAGE_SIZE - page_off;
    if (take > amount - copied) take = amount - copied;
    DirtyPage *dirty = get_dirty_page(page_no);
    if (!dirty) return write_failed ? write_failed : SQLITE_FULL;
    memcpy(dirty->data + page_off, src + copied, (usize)take);
    copied += take;
  }
  if (offset + amount > main_file_size) main_file_size = offset + amount;
  return SQLITE_OK;
}

static int mem_read(OctraFile *f, void *buf, int amount, sqlite3_int64 offset) {
  u8 *dst = (u8 *)buf;
  memset(dst, 0, (usize)amount);
  if (offset >= f->mem_size) return SQLITE_IOERR_SHORT_READ;
  int take = amount;
  if (offset + take > f->mem_size) take = (int)(f->mem_size - offset);
  memcpy(dst, f->mem + offset, (usize)take);
  return take == amount ? SQLITE_OK : SQLITE_IOERR_SHORT_READ;
}

static int mem_grow(OctraFile *f, sqlite3_int64 need) {
  if (need <= f->mem_cap) return SQLITE_OK;
  sqlite3_int64 cap = f->mem_cap ? f->mem_cap : 4096;
  while (cap < need) cap *= 2;
  u8 *next = (u8 *)realloc(f->mem, (usize)cap);
  if (!next) return SQLITE_NOMEM;
  if (cap > f->mem_cap) memset(next + f->mem_cap, 0, (usize)(cap - f->mem_cap));
  f->mem = next;
  f->mem_cap = cap;
  return SQLITE_OK;
}

static int mem_write(OctraFile *f, const void *buf, int amount, sqlite3_int64 offset) {
  sqlite3_int64 end = offset + amount;
  int rc = mem_grow(f, end);
  if (rc != SQLITE_OK) return rc;
  memcpy(f->mem + offset, buf, (usize)amount);
  if (end > f->mem_size) f->mem_size = end;
  return SQLITE_OK;
}

static int octra_close(sqlite3_file *file) {
  (void)file;
  return SQLITE_OK;
}

static int octra_read(sqlite3_file *file, void *buf, int amount, sqlite3_int64 offset) {
  OctraFile *f = (OctraFile *)file;
  if (f->is_main) return main_read(buf, amount, offset);
  return mem_read(f, buf, amount, offset);
}

static int octra_write(sqlite3_file *file, const void *buf, int amount, sqlite3_int64 offset) {
  OctraFile *f = (OctraFile *)file;
  if (f->is_main && f->readonly) return SQLITE_READONLY;
  if (f->is_main) return main_write(buf, amount, offset);
  return mem_write(f, buf, amount, offset);
}

static int octra_truncate(sqlite3_file *file, sqlite3_int64 size) {
  OctraFile *f = (OctraFile *)file;
  if (f->is_main) {
    if (f->readonly) return SQLITE_READONLY;
    main_file_size = size;
    return SQLITE_OK;
  }
  int rc = mem_grow(f, size);
  if (rc != SQLITE_OK) return rc;
  f->mem_size = size;
  return SQLITE_OK;
}

static int octra_sync(sqlite3_file *file, int flags) {
  (void)file;
  (void)flags;
  return SQLITE_OK;
}

static int octra_file_size(sqlite3_file *file, sqlite3_int64 *size) {
  OctraFile *f = (OctraFile *)file;
  *size = f->is_main ? main_file_size : f->mem_size;
  return SQLITE_OK;
}

static int octra_lock(sqlite3_file *file, int lock_type) {
  (void)file;
  (void)lock_type;
  return SQLITE_OK;
}

static int octra_unlock(sqlite3_file *file, int lock_type) {
  (void)file;
  (void)lock_type;
  return SQLITE_OK;
}

static int octra_check_reserved_lock(sqlite3_file *file, int *out) {
  (void)file;
  *out = 0;
  return SQLITE_OK;
}

static int octra_file_control(sqlite3_file *file, int op, void *arg) {
  (void)file;
  (void)op;
  (void)arg;
  return SQLITE_NOTFOUND;
}

static int octra_sector_size(sqlite3_file *file) {
  (void)file;
  return PAGE_SIZE;
}

static int octra_device_characteristics(sqlite3_file *file) {
  (void)file;
  return 0;
}

static const sqlite3_io_methods octra_io = {
  1,
  octra_close,
  octra_read,
  octra_write,
  octra_truncate,
  octra_sync,
  octra_file_size,
  octra_lock,
  octra_unlock,
  octra_check_reserved_lock,
  octra_file_control,
  octra_sector_size,
  octra_device_characteristics
};

static int octra_vfs_open(sqlite3_vfs *vfs, const char *name, sqlite3_file *file, int flags, int *out_flags) {
  (void)vfs;
  (void)name;
  OctraFile *octra_file = (OctraFile *)file;
  memset(octra_file, 0, sizeof(OctraFile));
  octra_file->base.pMethods = &octra_io;
  octra_file->is_main = (flags & SQLITE_OPEN_MAIN_DB) ? 1 : 0;
  octra_file->readonly = (flags & SQLITE_OPEN_READONLY) ? 1 : 0;
  if (out_flags) *out_flags = flags;
  return SQLITE_OK;
}

static int octra_vfs_delete(sqlite3_vfs *vfs, const char *name, int sync_dir) {
  (void)vfs;
  (void)name;
  (void)sync_dir;
  return SQLITE_OK;
}

static int octra_vfs_access(sqlite3_vfs *vfs, const char *name, int flags, int *out) {
  (void)vfs;
  (void)flags;
  const char *base = name ? strrchr(name, '/') : (char *)0;
  if (base) name = base + 1;
  if (name && strcmp(name, "octra-main.sqlite") != 0) {
    *out = 0;
    return SQLITE_OK;
  }
  int rc = load_meta();
  *out = (rc == SQLITE_OK && meta_exists) ? 1 : 0;
  return SQLITE_OK;
}

static int octra_vfs_full_pathname(sqlite3_vfs *vfs, const char *name, int out_len, char *out_path) {
  (void)vfs;
  int i = 0;
  if (!name) name = "octra-main.sqlite";
  while (i + 1 < out_len && name[i]) {
    out_path[i] = name[i];
    ++i;
  }
  out_path[i] = 0;
  return SQLITE_OK;
}

static void *octra_vfs_dl_open(sqlite3_vfs *vfs, const char *name) {
  (void)vfs;
  (void)name;
  return (void *)0;
}

static void octra_vfs_dl_error(sqlite3_vfs *vfs, int n, char *out_path) {
  (void)vfs;
  if (n > 0) out_path[0] = 0;
}

static void (*octra_vfs_dl_sym(sqlite3_vfs *vfs, void *handle, const char *symbol))(void) {
  (void)vfs;
  (void)handle;
  (void)symbol;
  return (void (*)(void))0;
}

static void octra_vfs_dl_close(sqlite3_vfs *vfs, void *handle) {
  (void)vfs;
  (void)handle;
}

static int octra_vfs_randomness(sqlite3_vfs *vfs, int n, char *out_path) {
  (void)vfs;
  for (int i = 0; i < n; ++i) out_path[i] = (char)((i * 1103515245u + 12345u) & 0xff);
  return n;
}

static int octra_vfs_sleep(sqlite3_vfs *vfs, int microseconds) {
  (void)vfs;
  return microseconds;
}

static int octra_vfs_current_time(sqlite3_vfs *vfs, double *out_time) {
  (void)vfs;
  *out_time = FIXED_JULIAN_DAY;
  return SQLITE_OK;
}

static int octra_vfs_get_last_error(sqlite3_vfs *vfs, int n, char *out_path) {
  (void)vfs;
  if (n > 0) out_path[0] = 0;
  return 0;
}

static int octra_vfs_current_time_int64(sqlite3_vfs *vfs, sqlite3_int64 *out_time) {
  (void)vfs;
  *out_time = FIXED_JULIAN_MS;
  return SQLITE_OK;
}

static sqlite3_vfs octra_vfs = {
  3,
  sizeof(OctraFile),
  1024,
  0,
  "octra_page_vfs",
  0,
  octra_vfs_open,
  octra_vfs_delete,
  octra_vfs_access,
  octra_vfs_full_pathname,
  octra_vfs_dl_open,
  octra_vfs_dl_error,
  octra_vfs_dl_sym,
  octra_vfs_dl_close,
  octra_vfs_randomness,
  octra_vfs_sleep,
  octra_vfs_current_time,
  octra_vfs_get_last_error,
  octra_vfs_current_time_int64,
  0,
  0,
  0
};

int sqlite3_os_init(void) {
  return sqlite3_vfs_register(&octra_vfs, 1);
}

int sqlite3_os_end(void) {
  return SQLITE_OK;
}

static void reset_runtime(void) {
  heap_pos = 0;
  reset_output();
  dirty_count = 0;
  main_file_size = 0;
  committed_file_size = 0;
  current_generation = 0;
  current_owner_sequence = 0;
  pending_owner_sequence = 0;
  pending_owner_sequence_active = 0;
  meta_loaded = 0;
  meta_exists = 0;
  meta_version = META_NONE;
  manifest_loaded = 0;
  manifest_page_count = 0;
  write_failed = 0;
}

static int deny_unsafe_sql(void *unused, int action, const char *arg1, const char *arg2, const char *db_name, const char *trigger_name) {
  (void)unused;
  (void)arg1;
  (void)arg2;
  (void)db_name;
  (void)trigger_name;
  switch (action) {
    case SQLITE_ATTACH:
    case SQLITE_DETACH:
    case SQLITE_PRAGMA:
    case SQLITE_TRANSACTION:
    case SQLITE_SAVEPOINT:
      return SQLITE_DENY;
    default:
      return SQLITE_OK;
  }
}

static void apply_sqlite_limits(sqlite3 *db) {
  sqlite3_limit(db, SQLITE_LIMIT_LENGTH, 1024 * 1024);
  sqlite3_limit(db, SQLITE_LIMIT_SQL_LENGTH, MAX_SQL_BYTES);
  sqlite3_limit(db, SQLITE_LIMIT_COLUMN, 128);
  sqlite3_limit(db, SQLITE_LIMIT_EXPR_DEPTH, 100);
  sqlite3_limit(db, SQLITE_LIMIT_COMPOUND_SELECT, 16);
  sqlite3_limit(db, SQLITE_LIMIT_VDBE_OP, 100000);
  sqlite3_limit(db, SQLITE_LIMIT_FUNCTION_ARG, 32);
  sqlite3_limit(db, SQLITE_LIMIT_ATTACHED, 0);
  sqlite3_limit(db, SQLITE_LIMIT_LIKE_PATTERN_LENGTH, 256);
  sqlite3_limit(db, SQLITE_LIMIT_VARIABLE_NUMBER, 64);
  sqlite3_limit(db, SQLITE_LIMIT_TRIGGER_DEPTH, 8);
  sqlite3_limit(db, SQLITE_LIMIT_WORKER_THREADS, 0);
  sqlite3_limit(db, SQLITE_LIMIT_PARSER_DEPTH, 100);
}

static int open_db(int readonly, sqlite3 **out_db) {
  sqlite3 *db = 0;
  int rc = load_meta();
  if (rc != SQLITE_OK) {
    append_json_error("circle_vfs_meta_failed", "could not read page VFS metadata");
    return rc;
  }
  int flags = (readonly && meta_exists)
      ? SQLITE_OPEN_READONLY
      : (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
  rc = sqlite3_open_v2(
      "octra-main.sqlite",
      &db,
      flags,
      "octra_page_vfs");
  if (rc != SQLITE_OK) {
    append_json_error("sqlite_open_failed", db ? sqlite3_errmsg(db) : "no db");
    if (db) sqlite3_close(db);
    return rc ? rc : 1;
  }
  apply_sqlite_limits(db);
#ifdef SQLITE_DBCONFIG_DEFENSIVE
  sqlite3_db_config(db, SQLITE_DBCONFIG_DEFENSIVE, 1, 0);
#endif
  char *err = 0;
  if (readonly && meta_exists) {
    rc = sqlite3_exec(
        db,
        "pragma temp_store=MEMORY;"
        "pragma query_only=ON;",
        0,
        0,
        &err);
  } else {
    rc = sqlite3_exec(
        db,
        "pragma page_size=4096;"
        "pragma journal_mode=MEMORY;"
        "pragma locking_mode=EXCLUSIVE;"
        "pragma temp_store=MEMORY;"
        "pragma secure_delete=ON;"
        "pragma max_page_count=" MAX_DB_PAGES_JSON ";",
        0,
        0,
        &err);
  }
  if (rc != SQLITE_OK) {
    append_json_error("sqlite_pragma_failed", err ? err : sqlite3_errmsg(db));
    sqlite3_free(err);
    sqlite3_close(db);
    return rc;
  }
  *out_db = db;
  return SQLITE_OK;
}

static int append_sql_value(sqlite3_stmt *stmt, int col) {
  int type = sqlite3_column_type(stmt, col);
  if (type == SQLITE_NULL) {
    append_cstr("null");
  } else if (type == SQLITE_INTEGER) {
    append_i64(sqlite3_column_int64(stmt, col));
  } else if (type == SQLITE_TEXT) {
    append_json_string_bytes(sqlite3_column_text(stmt, col), sqlite3_column_bytes(stmt, col));
  } else {
    return 1;
  }
  return out_overflow ? 2 : 0;
}

static int append_typed_sql_value(sqlite3_stmt *stmt, int col) {
  int type = sqlite3_column_type(stmt, col);
  if (type == SQLITE_NULL) {
    append_byte(0);
  } else if (type == SQLITE_INTEGER) {
    append_byte(1);
    append_be64_value((u64)sqlite3_column_int64(stmt, col));
  } else if (type == SQLITE_FLOAT) {
    union {
      double d;
      u64 u;
    } value;
    value.d = sqlite3_column_double(stmt, col);
    append_byte(2);
    append_be64_value(value.u);
  } else if (type == SQLITE_TEXT) {
    int n = sqlite3_column_bytes(stmt, col);
    append_byte(3);
    append_be32_value((u32)n);
    append_bytes(sqlite3_column_text(stmt, col), (u32)n);
  } else if (type == SQLITE_BLOB) {
    int n = sqlite3_column_bytes(stmt, col);
    append_byte(4);
    append_be32_value((u32)n);
    append_bytes((const u8 *)sqlite3_column_blob(stmt, col), (u32)n);
  } else {
    return 1;
  }
  return out_overflow ? 2 : 0;
}

static int run_sqlite_query(const char *sql) {
  sqlite3 *db = 0;
  sqlite3_stmt *stmt = 0;
  const char *tail = 0;
  int rc = open_db(1, &db);
  if (rc != SQLITE_OK) return 1;
  sqlite3_set_authorizer(db, deny_unsafe_sql, 0);
  rc = sqlite3_prepare_v2(db, sql, -1, &stmt, &tail);
  if (rc != SQLITE_OK) {
    append_json_error("sqlite_prepare_failed", sqlite3_errmsg(db));
    sqlite3_close(db);
    return 1;
  }
  if (!stmt) {
    append_json_error("sqlite_empty_query", "query did not produce a SQLite statement");
    sqlite3_close(db);
    return 1;
  }
  tail = skip_sql_tail(tail);
  if (tail && *tail) {
    append_json_error("sqlite_single_query_required", "query accepts one read-only SQLite statement");
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return 1;
  }
  if (!sqlite3_stmt_readonly(stmt)) {
    append_json_error("sqlite_readonly_required", "use exec for state-changing SQL");
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return 1;
  }
  int cols = sqlite3_column_count(stmt);
  append_result_envelope_open();
  append_cstr(",\"columns\":[");
  for (int i = 0; i < cols; ++i) {
    if (i) append_byte(',');
    append_json_string(sqlite3_column_name(stmt, i));
  }
  append_cstr("],\"rows\":[");
  while ((rc = sqlite3_step(stmt)) == SQLITE_ROW) {
    if (row_count >= MAX_RESULT_ROWS) {
      sqlite3_finalize(stmt);
      sqlite3_close(db);
      return set_json_error("result_limit_exceeded", "query returned too many rows");
    }
    if (row_count++) append_byte(',');
    append_byte('[');
    for (int i = 0; i < cols; ++i) {
      if (i) append_byte(',');
      int value_rc = append_sql_value(stmt, i);
      if (value_rc == 1) {
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        return set_json_error("unsupported_result_type", "REAL and BLOB result values need a typed result codec");
      }
      if (value_rc == 2) {
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        return set_json_error("response_too_large", "query result exceeded contract response buffer");
      }
    }
    append_byte(']');
    if (out_overflow) {
      sqlite3_finalize(stmt);
      sqlite3_close(db);
      return set_json_error("response_too_large", "query result exceeded contract response buffer");
    }
  }
  if (rc != SQLITE_DONE) {
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return set_json_error("sqlite_step_failed", "SQLite query did not finish cleanly");
  }
  append_cstr("],\"row_count\":");
  append_i64(row_count);
  append_byte('}');
  sqlite3_finalize(stmt);
  sqlite3_close(db);
  return 0;
}

static int run_sqlite_query_typed(const char *sql) {
  sqlite3 *db = 0;
  sqlite3_stmt *stmt = 0;
  const char *tail = 0;
  int rc = open_db(1, &db);
  if (rc != SQLITE_OK) return 1;
  sqlite3_set_authorizer(db, deny_unsafe_sql, 0);
  rc = sqlite3_prepare_v2(db, sql, -1, &stmt, &tail);
  if (rc != SQLITE_OK) {
    append_json_error("sqlite_prepare_failed", sqlite3_errmsg(db));
    sqlite3_close(db);
    return 1;
  }
  if (!stmt) {
    append_json_error("sqlite_empty_query", "query did not produce a SQLite statement");
    sqlite3_close(db);
    return 1;
  }
  tail = skip_sql_tail(tail);
  if (tail && *tail) {
    append_json_error("sqlite_single_query_required", "query accepts one read-only SQLite statement");
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return 1;
  }
  if (!sqlite3_stmt_readonly(stmt)) {
    append_json_error("sqlite_readonly_required", "use exec for state-changing SQL");
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return 1;
  }

  int cols = sqlite3_column_count(stmt);
  append_bytes((const u8 *)"OSR1", 4);
  append_be32_value((u32)cols);
  u32 row_count_offset = out_len;
  append_be32_value(0);
  for (int i = 0; i < cols; ++i) {
    const char *name = sqlite3_column_name(stmt, i);
    u32 n = name ? (u32)strlen(name) : 0u;
    append_be32_value(n);
    append_bytes((const u8 *)name, n);
  }

  while ((rc = sqlite3_step(stmt)) == SQLITE_ROW) {
    if (row_count >= MAX_RESULT_ROWS) {
      sqlite3_finalize(stmt);
      sqlite3_close(db);
      return set_json_error("result_limit_exceeded", "query returned too many rows");
    }
    ++row_count;
    for (int i = 0; i < cols; ++i) {
      int value_rc = append_typed_sql_value(stmt, i);
      if (value_rc == 1) {
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        return set_json_error("unsupported_result_type", "SQLite returned an unknown result type");
      }
      if (value_rc == 2) {
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        return set_json_error("response_too_large", "typed query result exceeded contract response buffer");
      }
    }
  }
  if (rc != SQLITE_DONE) {
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return set_json_error("sqlite_step_failed", "SQLite query did not finish cleanly");
  }
  patch_be32(row_count_offset, (u32)row_count);
  sqlite3_finalize(stmt);
  sqlite3_close(db);
  return out_overflow ? set_json_error("response_too_large", "typed query result exceeded contract response buffer") : 0;
}

static int run_schema(void) {
  return run_sqlite_query("select type, name, sql from sqlite_master where type in ('table','view','index','trigger') order by type, name;");
}

static int run_schema_typed(void) {
  return run_sqlite_query_typed("select type, name, sql from sqlite_master where type in ('table','view','index','trigger') order by type, name;");
}

static int run_sqlite_exec(const char *sql, int trace_sql) {
  sqlite3 *db = 0;
  char *err = 0;
  int rc = open_db(0, &db);
  if (rc != SQLITE_OK) return 1;

  rc = sqlite3_exec(db, "begin immediate;", 0, 0, &err);
  if (rc != SQLITE_OK) {
    append_json_error("sqlite_begin_failed", err ? err : sqlite3_errmsg(db));
    sqlite3_free(err);
    sqlite3_close(db);
    return 1;
  }

  int before = sqlite3_total_changes(db);
  sqlite3_set_authorizer(db, deny_unsafe_sql, 0);
  rc = sqlite3_exec(db, sql, 0, 0, &err);
  sqlite3_set_authorizer(db, 0, 0);
  if (rc == SQLITE_OK) {
    rc = sqlite3_exec(db, "commit;", 0, 0, &err);
  }
  if (rc != SQLITE_OK) {
    sqlite3_exec(db, "rollback;", 0, 0, 0);
    const char *detail = err ? err : sqlite3_errmsg(db);
    emit_sql_error_event("sqlite_exec_failed", detail);
    append_json_error("sqlite_exec_failed", detail);
    sqlite3_free(err);
    sqlite3_close(db);
    return OCTRA_SQLITE_APP_ERROR;
  }

  int changes = sqlite3_total_changes(db) - before;
  rc = flush_dirty_pages();
  if (rc != SQLITE_OK) {
    append_json_error("circle_vfs_flush_failed", "could not flush dirty SQLite pages to Circle key-value storage");
    sqlite3_close(db);
    return 1;
  }
  sqlite3_close(db);
  emit_exec_event(sql, trace_sql);
  append_result_envelope_open();
  append_cstr(",\"persisted\":true,");
  append_cstr("\"dirty_pages\":");
  append_i64(dirty_count);
  append_cstr(",\"generation\":");
  append_i64((i64)current_generation);
  append_cstr(",\"file_bytes\":");
  append_i64((i64)main_file_size);
  append_cstr(",\"changes\":");
  append_i64(changes);
  append_byte('}');
  return 0;
}

static int run_storage_info(void) {
  int rc = load_meta();
  if (rc != SQLITE_OK) {
    append_json_error("circle_vfs_meta_failed", "could not read page VFS metadata");
    return 1;
  }
  sqlite3_int64 page_count = page_count_for_size(main_file_size);
  append_cstr("{\"ok\":true,\"storage\":\"");
  append_cstr(STORAGE_ID);
  append_cstr("\",\"meta_key\":");
  append_json_string(meta_key);
  append_cstr(",\"exists\":");
  append_cstr(meta_exists ? "true" : "false");
  append_cstr(",\"page_size\":");
  append_cstr(PAGE_SIZE_JSON);
  append_cstr(",\"file_bytes\":");
  append_i64((i64)main_file_size);
  append_cstr(",\"page_count\":");
  append_i64((i64)page_count);
  append_cstr(",\"generation\":");
  append_i64((i64)current_generation);
  append_cstr(",\"commit_protocol\":\"generation_manifest_v4\"");
  append_cstr(",\"meta_version\":");
  append_i64(meta_version);
  append_cstr(",\"owner_sequence\":");
  append_i64((i64)current_owner_sequence);
  append_cstr(",\"max_dirty_pages\":");
  append_i64(MAX_DIRTY_PAGES);
  append_cstr(",\"max_db_pages\":");
  append_i64(MAX_DB_PAGES);
  append_byte('}');
  return 0;
}

static int auth_configured(void);

static int run_auth_info(void) {
  int rc = load_meta();
  if (rc != SQLITE_OK) {
    append_json_error("circle_vfs_meta_failed", "could not read page VFS metadata");
    return 1;
  }
  append_result_envelope_open();
  append_cstr(",\"auth\":\"osw1\",");
  append_cstr("\"configured\":");
  append_cstr(auth_configured() ? "true" : "false");
  append_cstr(",\"db_id\":\"");
  append_hex_bytes(configured_db_id, 32);
  append_cstr("\",\"owner_pubkey\":\"");
  append_hex_bytes(configured_owner_pubkey, 32);
  append_cstr("\",\"owner_sequence\":");
  append_i64((i64)current_owner_sequence);
  append_byte('}');
  return 0;
}

static int reset_storage(void) {
  int rc = load_meta();
  if (rc == SQLITE_OK) {
    rc = load_manifest();
    if (rc == SQLITE_OK) {
      for (sqlite3_int64 i = 0; i < manifest_page_count; ++i) {
        delete_page_version((u32)i + 1u, page_generations[i]);
      }
      if (meta_version == META_MANIFEST_GENERATION_V3 || meta_version == META_MANIFEST_WITH_AUTH_V4) {
        delete_manifest(current_generation);
      }
    }
  }
  int code = host_kv_del((const u8 *)meta_key, (int)sizeof(meta_key) - 1);
  if (code < 0 && code != -1) {
    append_json_error("circle_vfs_reset_failed", "could not delete page VFS metadata");
    return 1;
  }
  append_cstr("{\"ok\":true,\"storage\":\"");
  append_cstr(STORAGE_ID);
  append_cstr("\",\"reset\":true}");
  return 0;
}

__attribute__((export_name("octra_manifest")))
int octra_manifest(int ptr, int len) {
  (void)ptr;
  (void)len;
  return respond_manifest();
}

static int parse_method_call(int ptr, int len, const u8 **method, u32 *method_len, const u8 **params, u32 *param_count) {
  const u8 *p = (const u8 *)ptr;
  if (len < 9) return OCTRA_ERR_FRAME_TOO_SHORT;
  if (p[0] != 'O' || p[1] != 'C' || p[2] != 'W' || p[3] != 'R' || p[4] != '1') return OCTRA_ERR_BAD_FRAME_MAGIC;
  u32 off = 5;
  *method_len = be16(p + off);
  off += 2;
  if (off + *method_len + 2 > (u32)len) return OCTRA_ERR_FRAME_BOUNDS;
  *method = p + off;
  off += *method_len;
  *param_count = be16(p + off);
  off += 2;
  *params = p + off;
  return 0;
}

typedef struct StringParam StringParam;
struct StringParam {
  const u8 *ptr;
  u32 len;
};

static int parse_string_params(const u8 *params, u32 param_count, int total_len, int ptr, StringParam *out, u32 expected) {
  if (param_count != expected) return OCTRA_ERR_PARAM_COUNT;
  const u8 *p = params;
  const u8 *end = (const u8 *)ptr + total_len;
  for (u32 i = 0; i < expected; ++i) {
    if (p + 5 > end) return OCTRA_ERR_FRAME_BOUNDS;
    u8 tag = *p++;
    u32 value_len = be32(p);
    p += 4;
    if (tag != 4) return OCTRA_ERR_BAD_PARAM_TYPE;
    if (p + value_len > end) return OCTRA_ERR_FRAME_BOUNDS;
    out[i].ptr = p;
    out[i].len = value_len;
    p += value_len;
  }
  return 0;
}

static int parse_one_string_param(const u8 *params, u32 param_count, int total_len, int ptr, char *sql) {
  StringParam p[1];
  int rc = parse_string_params(params, param_count, total_len, ptr, p, 1);
  if (rc != 0) return rc;
  if (p[0].len >= MAX_SQL_BYTES) return OCTRA_ERR_FRAME_BOUNDS;
  memcpy(sql, p[0].ptr, p[0].len);
  sql[p[0].len] = 0;
  return 0;
}

static int match4(const u8 value[32], u32 off, u8 a, u8 b, u8 c, u8 d) {
  return value[off] == a && value[off + 1u] == b && value[off + 2u] == c && value[off + 3u] == d;
}

static int owner_pubkey_is_placeholder(void) {
  return match4(configured_owner_pubkey, 0, 'O', 'S', 'Q', 'L') &&
         match4(configured_owner_pubkey, 4, '_', 'O', 'W', 'N') &&
         match4(configured_owner_pubkey, 8, 'E', 'R', '_', 'P') &&
         match4(configured_owner_pubkey, 12, 'U', 'B', 'K', 'E') &&
         match4(configured_owner_pubkey, 16, 'Y', '_', 'V', '1') &&
         match4(configured_owner_pubkey, 20, '_', 'P', 'L', 'A') &&
         match4(configured_owner_pubkey, 24, 'C', 'E', 'H', 'O') &&
         match4(configured_owner_pubkey, 28, 'L', 'D', 'E', 'R');
}

static int db_id_is_placeholder(void) {
  return match4(configured_db_id, 0, 'O', 'S', 'Q', 'L') &&
         match4(configured_db_id, 4, '_', 'D', 'A', 'T') &&
         match4(configured_db_id, 8, 'A', 'B', 'A', 'S') &&
         match4(configured_db_id, 12, 'E', '_', 'I', 'D') &&
         match4(configured_db_id, 16, '_', 'V', '1', '_') &&
         match4(configured_db_id, 20, 'P', 'L', 'A', 'C') &&
         match4(configured_db_id, 24, 'E', 'H', 'O', 'L') &&
         match4(configured_db_id, 28, 'D', 'E', 'R', '0');
}

static int auth_configured(void) {
  return !owner_pubkey_is_placeholder() && !db_id_is_placeholder();
}

static int hex_value(u8 ch) {
  if (ch >= '0' && ch <= '9') return (int)(ch - '0');
  if (ch >= 'a' && ch <= 'f') return 10 + (int)(ch - 'a');
  if (ch >= 'A' && ch <= 'F') return 10 + (int)(ch - 'A');
  return -1;
}

static int decode_hex_exact(const u8 *text, u32 text_len, u8 *out, u32 out_len) {
  if (text_len != out_len * 2u) return 0;
  for (u32 i = 0; i < out_len; ++i) {
    int hi = hex_value(text[i * 2u]);
    int lo = hex_value(text[i * 2u + 1u]);
    if (hi < 0 || lo < 0) return 0;
    out[i] = (u8)((hi << 4) | lo);
  }
  return 1;
}

static int parse_u64_text(const u8 *text, u32 len, u64 *out) {
  if (len == 0 || len > 20u) return 0;
  u64 value = 0;
  for (u32 i = 0; i < len; ++i) {
    if (text[i] < '0' || text[i] > '9') return 0;
    u64 digit = (u64)(text[i] - '0');
    if (value > (18446744073709551615ull - digit) / 10ull) return 0;
    value = value * 10ull + digit;
  }
  *out = value;
  return 1;
}

static int build_owner_write_intent_message(const char *sql, const u8 *method, u32 method_len, u64 sequence, u8 *out_msg, u32 *out_len) {
  u32 sql_len = (u32)strlen(sql);
  if (method_len == 0 || method_len > MAX_METHOD_BYTES) return 1;
  u32 len = OWNER_WRITE_INTENT_DOMAIN_LEN + 32u + 8u + 2u + method_len + 4u + sql_len;
  if (len > MAX_OWNER_WRITE_INTENT_BYTES) return 1;
  u32 w = 0;
  memcpy(out_msg + w, OWNER_WRITE_INTENT_DOMAIN, OWNER_WRITE_INTENT_DOMAIN_LEN); w += OWNER_WRITE_INTENT_DOMAIN_LEN;
  memcpy(out_msg + w, configured_db_id, 32); w += 32;
  put_be64(out_msg + w, sequence); w += 8;
  put_be16(out_msg + w, method_len); w += 2;
  memcpy(out_msg + w, method, method_len); w += method_len;
  put_be32(out_msg + w, sql_len); w += 4;
  memcpy(out_msg + w, sql, sql_len); w += sql_len;
  *out_len = w;
  return 0;
}

static int verify_signed_exec_params(const u8 *params, u32 param_count, int total_len, int ptr, const u8 *method, u32 method_len, char *sql) {
  if (!auth_configured()) {
    append_auth_error("auth_not_configured", "signed exec requires owner-patched Circle WASM");
    return 1;
  }
  if (param_count != 4) {
    append_auth_error("auth_required", "exec requires signed OSW1 write intent");
    return 1;
  }
  StringParam p[4];
  int rc = parse_string_params(params, param_count, total_len, ptr, p, 4);
  if (rc != 0) return rc;
  if (p[0].len >= MAX_SQL_BYTES) return OCTRA_ERR_FRAME_BOUNDS;
  memcpy(sql, p[0].ptr, p[0].len);
  sql[p[0].len] = 0;

  u8 pubkey[32];
  u8 sig[64];
  if (!decode_hex_exact(p[1].ptr, p[1].len, pubkey, 32) ||
      !decode_hex_exact(p[3].ptr, p[3].len, sig, 64)) {
    append_auth_error("auth_bad_encoding", "public key or signature must be hex");
    return 1;
  }
  if (memcmp(pubkey, configured_owner_pubkey, 32) != 0) {
    append_auth_error("auth_denied", "signed exec signer is not the database owner");
    return 1;
  }

  u64 sequence = 0;
  if (!parse_u64_text(p[2].ptr, p[2].len, &sequence) || sequence == 0) {
    append_auth_error("auth_bad_sequence", "signed exec sequence must be a positive decimal u64");
    return 1;
  }

  static u8 msg[MAX_OWNER_WRITE_INTENT_BYTES];
  u32 msg_len = 0;
  if (build_owner_write_intent_message(sql, method, method_len, sequence, msg, &msg_len) != 0) {
    append_auth_error("auth_message_too_large", "signed exec message is too large");
    return 1;
  }
  static u8 signed_msg[64 + MAX_OWNER_WRITE_INTENT_BYTES];
  static u8 opened[MAX_OWNER_WRITE_INTENT_BYTES];
  memcpy(signed_msg, sig, 64);
  memcpy(signed_msg + 64, msg, msg_len);
  u64 opened_len = 0;
  if (crypto_sign_open(opened, &opened_len, signed_msg, (u64)msg_len + 64u, pubkey) != 0 ||
      opened_len != (u64)msg_len ||
      memcmp(opened, msg, msg_len) != 0) {
    append_auth_error("auth_bad_signature", "signed exec signature verification failed");
    return 1;
  }

  int meta_rc = load_meta();
  if (meta_rc != SQLITE_OK) {
    append_json_error("circle_vfs_meta_failed", "could not read page VFS metadata");
    return 1;
  }
  if (sequence <= current_owner_sequence) {
    append_auth_error("auth_replay", "signed exec sequence has already been used");
    return 1;
  }
  pending_owner_sequence = sequence;
  pending_owner_sequence_active = 1;
  return 0;
}

__attribute__((export_name("octra_query")))
int octra_query(int ptr, int len) {
  const u8 *method = 0;
  const u8 *params = 0;
  u32 method_len = 0;
  u32 param_count = 0;
  int rc = parse_method_call(ptr, len, &method, &method_len, &params, &param_count);
  if (rc != 0) return rc;

  if (streq_bytes(method, method_len, "health")) {
    reset_runtime();
    append_result_envelope_open();
    append_byte('}');
    return respond_string_bytes(out, out_len, 0);
  }

  reset_runtime();
  if (streq_bytes(method, method_len, "storage_info")) {
    rc = run_storage_info();
    return respond_json_result(0);
  }
  if (streq_bytes(method, method_len, "schema")) {
    rc = run_schema();
    return respond_json_result(0);
  }
  if (streq_bytes(method, method_len, "schema_typed")) {
    rc = run_schema_typed();
    return rc ? respond_json_result(0) : respond_typed_result(0);
  }
  if (streq_bytes(method, method_len, "auth_info")) {
    rc = run_auth_info();
    return respond_json_result(0);
  }
  int typed = 0;
  if (streq_bytes(method, method_len, "query_typed")) {
    typed = 1;
  } else if (!streq_bytes(method, method_len, "query")) {
    return OCTRA_ERR_UNKNOWN_METHOD;
  }

  static char sql[MAX_SQL_BYTES];
  rc = parse_one_string_param(params, param_count, len, ptr, sql);
  if (rc != 0) return rc;
  rc = typed ? run_sqlite_query_typed(sql) : run_sqlite_query(sql);
  return typed ? (rc ? respond_json_result(0) : respond_typed_result(0)) : respond_json_result(0);
}

__attribute__((export_name("octra_update")))
int octra_update(int ptr, int len) {
  const u8 *method = 0;
  const u8 *params = 0;
  u32 method_len = 0;
  u32 param_count = 0;
  int rc = parse_method_call(ptr, len, &method, &method_len, &params, &param_count);
  if (rc != 0) return rc;

  reset_runtime();
  if (streq_bytes(method, method_len, "reset")) {
    if (auth_configured()) {
      append_auth_error("auth_required", "reset requires signed admin support");
      return respond_json_result(auth_policy_status_code);
    }
    rc = reset_storage();
    return respond_json_result(rc ? 1 : 0);
  }
  int trace_sql = 0;
  if (streq_bytes(method, method_len, "exec_trace")) {
    trace_sql = 1;
  } else if (!streq_bytes(method, method_len, "exec")) {
    return OCTRA_ERR_UNKNOWN_METHOD;
  }

  static char sql[MAX_SQL_BYTES];
  if (auth_configured()) {
    rc = verify_signed_exec_params(params, param_count, len, ptr, method, method_len, sql);
    if (rc != 0) {
      if (out_len > 0) return respond_json_result(auth_policy_error ? auth_policy_status_code : 1);
      return rc;
    }
  } else {
    rc = parse_one_string_param(params, param_count, len, ptr, sql);
    if (rc != 0) return rc;
  }
  rc = run_sqlite_exec(sql, trace_sql);
  return respond_json_result(rc == OCTRA_SQLITE_APP_ERROR ? 0 : (rc ? 1 : 0));
}
