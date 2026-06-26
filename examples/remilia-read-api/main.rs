use octra_sqlite::client::{Database, HttpTransport, OctraSqlite, QueryResult};
use serde_json::{json, Map, Value};
use std::{
    env,
    error::Error,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
};

type AppResult<T> = Result<T, Box<dyn Error>>;

fn main() -> AppResult<()> {
    let addr = env::var("OCTRA_SQLITE_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let database_name = env::var("OCTRA_SQLITE_DATABASE").unwrap_or_else(|_| "remilia".to_string());
    let client = OctraSqlite::from_default_config()?;
    let database = client.database(database_name.clone())?;
    let listener = TcpListener::bind(&addr)?;

    eprintln!("listening on http://{addr}");
    eprintln!("try: curl http://{addr}/collections/milady");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, &database, &database_name) {
                    eprintln!("request error: {error}");
                }
            }
            Err(error) => eprintln!("accept error: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(
    mut stream: TcpStream,
    database: &Database<HttpTransport>,
    database_name: &str,
) -> AppResult<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    if method != "GET" {
        return write_json(
            &mut stream,
            "405 Method Not Allowed",
            &json!({"error":"method_not_allowed"}),
        );
    }

    if path == "/" || path == "/health" {
        return write_json(
            &mut stream,
            "200 OK",
            &json!({"ok":true,"database":database_name}),
        );
    }

    let Some(slug) = collection_slug(path) else {
        return write_json(
            &mut stream,
            "404 Not Found",
            &json!({"error":"not_found","routes":["GET /collections/<opensea_slug>"]}),
        );
    };

    let result = query_collection(database, &slug)?;
    write_json(
        &mut stream,
        "200 OK",
        &json!({
            "database": database_name,
            "collection": slug,
            "row_count": result.row_count,
            "rows": rows_as_objects(&result),
        }),
    )
}

fn query_collection(database: &Database<HttpTransport>, slug: &str) -> AppResult<QueryResult> {
    let sql = format!(
        "select name, opensea_slug, chain, relationship, launched_month, date_precision \
         from collection where opensea_slug = {} limit 1;",
        sql_quote(slug)
    );
    Ok(database.query(&sql)?)
}

fn collection_slug(path: &str) -> Option<String> {
    let path = path.split('?').next().unwrap_or(path);
    let slug = path.strip_prefix("/collections/")?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_string())
    }
}

fn rows_as_objects(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| {
            let mut object = Map::new();
            for (column, value) in result.columns.iter().zip(row) {
                object.insert(column.clone(), value.clone());
            }
            Value::Object(object)
        })
        .collect()
}

fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn write_json(stream: &mut TcpStream, status: &str, value: &Value) -> AppResult<()> {
    let body = serde_json::to_vec_pretty(value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}
