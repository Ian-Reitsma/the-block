use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use warp::{reject, Filter, Rejection};

#[derive(Clone)]
struct Rpc {
    url: String,
    client: Client,
}

fn with_rpc(rpc: Rpc) -> impl Filter<Extract = (Rpc,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rpc.clone())
}

#[derive(Deserialize)]
struct VoteForm {
    id: u64,
    house: String,
}

#[tokio::main]
async fn main() {
    let rpc = Rpc {
        url: std::env::var("TB_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".into()),
        client: Client::new(),
    };
    let list = warp::path::end()
        .and(with_rpc(rpc.clone()))
        .and_then(|rpc: Rpc| async move {
            let credit_req = json!({"method": "gov_credit_list"});
            let credit_resp: Value = rpc
                .client
                .post(&rpc.url)
                .json(&credit_req)
                .send()
                .await
                .map_err(|_| reject::reject())?
                .json()
                .await
                .map_err(|_| reject::reject())?;
            let credits: Vec<Value> =
                serde_json::from_value(credit_resp["result"].clone()).unwrap_or_default();

            let param_req = json!({"method": "gov_list"});
            let param_resp: Value = rpc
                .client
                .post(&rpc.url)
                .json(&param_req)
                .send()
                .await
                .map_err(|_| reject::reject())?
                .json()
                .await
                .map_err(|_| reject::reject())?;
            let params: Vec<Value> =
                serde_json::from_value(param_resp["result"].clone()).unwrap_or_default();

            let mut html = String::from("<html><head><style>body{background:#121212;color:#eee;font-family:sans-serif;}a{color:#8ab4f8;} input,select{background:#222;color:#eee;border:1px solid #555;padding:4px;} </style></head><body><h1>Credit Proposals</h1><ul>");
            for p in credits {
                let id = p["id"].as_u64().unwrap_or(0);
                let ops = p["ops_for"].as_u64().unwrap_or(0);
                let builders = p["builders_for"].as_u64().unwrap_or(0);
                let executed = p["executed"].as_bool().unwrap_or(false);
                let status = if executed { "✅" } else { "⏳" };
                if let Some(issue) = p.get("credit_issue") {
                    let prov = issue["provider"].as_str().unwrap_or("?");
                    let amt = issue["amount"].as_u64().unwrap_or(0);
                    html.push_str(&format!(
                        "<li>id={id} provider={prov} amount={amt} ops_for={ops} builders_for={builders} {status}</li>"
                    ));
                }
            }
            html.push_str("</ul><h2>Vote</h2><form method=post action=/vote>id:<input name=id type=number /> house:<select name=house><option>ops</option><option>builders</option></select><button type=submit>vote</button></form>");
            html.push_str("<h1>Param Proposals</h1><ul>");
            for p in params {
                let id = p["id"].as_u64().unwrap_or(0);
                let key = p["key"].as_str().unwrap_or("?");
                let val = p["new_value"].as_i64().unwrap_or(0);
                html.push_str(&format!("<li>id={id} key={key} new_value={val}</li>"));
            }
            html.push_str("</ul></body></html>");
            Ok::<_, Rejection>(warp::reply::html(html))
        });

    let vote = warp::post()
        .and(warp::path("vote"))
        .and(warp::body::form())
        .and(with_rpc(rpc.clone()))
        .and_then(|form: VoteForm, rpc: Rpc| async move {
            let body = json!({
                "method": "gov_credit_vote",
                "params": {"id": form.id, "house": form.house}
            });
            let _res: Value = rpc
                .client
                .post(&rpc.url)
                .json(&body)
                .send()
                .await
                .map_err(|_| reject::reject())?
                .json()
                .await
                .map_err(|_| reject::reject())?;
            Ok::<_, Rejection>(warp::redirect::see_other(warp::http::Uri::from_static("/")))
        });

    warp::serve(list.or(vote)).run(([127, 0, 0, 1], 8080)).await;
}

