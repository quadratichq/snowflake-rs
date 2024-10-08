extern crate snowflake_api;

use anyhow::Result;
use arrow::util::pretty::pretty_format_batches;
use clap::{ArgAction, Parser};
use futures_util::StreamExt;
use std::fs;

use snowflake_api::{responses::ExecResponse, QueryResult, RawQueryResult, SnowflakeApi};

#[derive(clap::ValueEnum, Clone, Debug)]
enum Output {
    Arrow,
    Json,
    Query,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to RSA PEM private key
    #[arg(long)]
    private_key: Option<String>,

    /// Password if certificate is not present
    #[arg(long)]
    password: Option<String>,

    /// <account_identifier> in Snowflake format, uppercase
    #[arg(short, long)]
    account_identifier: String,

    /// Database name
    #[arg(short, long)]
    database: Option<String>,

    /// Schema name
    #[arg(long)]
    schema: Option<String>,

    /// Warehouse
    #[arg(short, long)]
    warehouse: Option<String>,

    /// username to whom the private key belongs to
    #[arg(short, long)]
    username: String,

    /// role which user will assume
    #[arg(short, long)]
    role: Option<String>,

    /// sql statement to execute and print result from
    #[arg(long)]
    sql: String,

    #[arg(long)]
    #[arg(value_enum, default_value_t = Output::Arrow)]
    output: Output,

    #[arg(long)]
    host: Option<String>,

    #[clap(long, action = ArgAction::Set)]
    stream: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let args = Args::parse();

    let mut api = match (&args.private_key, &args.password) {
        (Some(pkey), None) => {
            let pem = fs::read_to_string(pkey)?;
            SnowflakeApi::with_certificate_auth(
                &args.account_identifier,
                args.warehouse.as_deref(),
                args.database.as_deref(),
                args.schema.as_deref(),
                &args.username,
                args.role.as_deref(),
                &pem,
            )?
        }
        (None, Some(pwd)) => SnowflakeApi::with_password_auth(
            &args.account_identifier,
            args.warehouse.as_deref(),
            args.database.as_deref(),
            args.schema.as_deref(),
            &args.username,
            args.role.as_deref(),
            pwd,
        )?,
        _ => {
            panic!("Either private key path or password must be set")
        }
    }
    // add optional host
    .with_host(args.host);

    if args.stream {
        let resp = api.exec_raw(&args.sql, true).await?;

        if let RawQueryResult::Stream(mut bytes_stream) = resp {
            let mut chunks = vec![];
            while let Some(bytes) = bytes_stream.next().await {
                chunks.push(bytes?);
            }

            let bytes = chunks.into_iter().flatten().collect::<Vec<u8>>();
            let resp = serde_json::from_slice::<ExecResponse>(&bytes).unwrap();
            let raw_query_result = api.parse_arrow_raw_response(resp).await.unwrap();
            let batches = raw_query_result.deserialize_arrow().unwrap();

            if let QueryResult::Arrow(a) = batches {
                println!("{}", pretty_format_batches(&a).unwrap());
            }
        }
    } else {
        match args.output {
            Output::Arrow => {
                let res = api.exec(&args.sql).await?;
                match res {
                    QueryResult::Arrow(a) => {
                        println!("{}", pretty_format_batches(&a).unwrap());
                    }
                    QueryResult::Json(j) => {
                        println!("{j}");
                    }
                    QueryResult::Empty => {
                        println!("Query finished successfully")
                    }
                }
            }
            Output::Json => {
                let res = api.exec_json(&args.sql).await?;
                println!("{res}");
            }
            Output::Query => {
                let res = api.exec_response(&args.sql).await?;
                println!("{:?}", res);
            }
        }
    }

    Ok(())
}
