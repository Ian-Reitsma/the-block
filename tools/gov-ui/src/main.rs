use serde::Deserialize;
use std::sync::{Arc, Mutex};
use the_block::{governance::House, Governance};
use warp::{Filter, Rejection};

fn with_gov(
    g: Arc<Mutex<Governance>>,
) -> impl Filter<Extract = (Arc<Mutex<Governance>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || g.clone())
}

#[derive(Deserialize)]
struct VoteForm {
    id: u64,
    house: String,
}

#[tokio::main]
async fn main() {
    let gov = Arc::new(Mutex::new(Governance::load(
        "examples/governance/proposals.db",
        1,
        1,
        0,
    )));
    let list = warp::path::end().and(with_gov(gov.clone())).map(|g: Arc<Mutex<Governance>>| {
        let gov = g.lock().unwrap();
        let proposals = gov.list();
        let mut html = String::from(
            "<html><head><style>body{background:#121212;color:#eee;font-family:sans-serif;}\na{color:#8ab4f8;} input,select{background:#222;color:#eee;border:1px solid #555;padding:4px;}\n</style></head><body><h1>Proposals</h1><ul>"
        );
        for p in proposals {
            let status = if p.executed { "✅" } else { "⏳" };
            html.push_str(&format!(
                "<li>id={} ops_for={} builders_for={} {status}</li>",
                p.id, p.ops_for, p.builders_for
            ));
        }
        html.push_str("</ul><h2>Vote</h2><form method=post action=/vote>id:<input name=id type=number /> house:<select name=house><option>ops</option><option>builders</option></select><button type=submit>vote</button></form></body></html>");
        warp::reply::html(html)
    });
    let vote = warp::post()
        .and(warp::path("vote"))
        .and(warp::body::form())
        .and(with_gov(gov.clone()))
        .and_then(|form: VoteForm, g: Arc<Mutex<Governance>>| async move {
            let mut gov = g.lock().unwrap();
            let house = match form.house.as_str() {
                "ops" => House::Operators,
                _ => House::Builders,
            };
            let _ = gov.vote(form.id, house, true);
            let _ = gov.persist("examples/governance/proposals.db");
            Ok::<_, Rejection>(warp::redirect::see_other(warp::http::Uri::from_static("/")))
        });
    warp::serve(list.or(vote)).run(([127, 0, 0, 1], 8080)).await;
}
