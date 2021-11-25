use serde::Deserialize;

use std::net::Ipv6Addr;

use actix_web::{
    get, post,
    web::{self, Bytes},
    App, HttpResponse, HttpServer, Responder,
};

#[actix_web::main]
pub async fn use_doh(bind_address: Ipv6Addr, bind_port: u16) -> std::io::Result<()> {
    HttpServer::new(|| App::new().service(doh_post).service(doh_get))
        .bind((bind_address, bind_port))?
        .run()
        .await
}

#[post("/dns-query")]
async fn doh_post(body: Bytes) -> impl Responder {
    HttpResponse::Ok()
        .content_type("application/dns-message")
        .body("Hello world!")
}

#[derive(Deserialize)]
struct GetQueries {
    dns: String,
}

#[get("/dns-query")]
async fn doh_get(queries: web::Query<GetQueries>) -> impl Responder {
    let query_b64 = &queries.dns;

    HttpResponse::Ok()
        .content_type("application/dns-message")
        .body("Hello world!")
}
