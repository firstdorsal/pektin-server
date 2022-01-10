use crate::{process_request, PektinResult, ProcessedRequest};
use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use pektin_common::deadpool_redis::Pool;
use pektin_common::proto::op::Message;
use serde::Deserialize;
use std::net::Ipv6Addr;

pub async fn use_doh(
    bind_address: Ipv6Addr,
    bind_port: u16,
    redis_pool: Pool,
) -> PektinResult<Server> {
    Ok(HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allowed_header("content-type")
                    .allowed_methods(vec!["GET", "POST"]),
            )
            .app_data(web::Data::new(redis_pool.clone()))
            .service(doh_post)
            .service(doh_get)
    })
    .bind((bind_address, bind_port))?
    .run())
}

#[post("/dns-query")]
async fn doh_post(body: web::Bytes, redis_pool: web::Data<Pool>) -> impl Responder {
    eprintln!("uwu~");
    let bytes: Vec<u8> = body.into_iter().collect();
    let message = match Message::from_vec(&bytes) {
        Ok(m) => m,
        _ => {
            return HttpResponse::BadRequest()
                .content_type("application/dns-message")
                .body("")
        }
    };

    match process_request_doh(message, redis_pool.get_ref().clone()).await {
        Some(bytes) => HttpResponse::Ok()
            .content_type("application/dns-message")
            .body(bytes),
        None => HttpResponse::BadRequest()
            .content_type("application/dns-message")
            .body("Bad request"),
    }
}

#[derive(Deserialize)]
struct GetQueries {
    dns: String,
}

#[get("/dns-query")]
async fn doh_get(queries: web::Query<GetQueries>, _redis_pool: web::Data<Pool>) -> impl Responder {
    eprintln!("owo~");
    let _query_b64 = &queries.dns;

    HttpResponse::Ok()
        .content_type("application/dns-message")
        .body("hi there")
}

async fn process_request_doh(message: Message, redis_pool: Pool) -> Option<Vec<u8>> {
    let processed_request = process_request(message, redis_pool).await;
    if let ProcessedRequest::Handled(response) = processed_request {
        response.to_vec().ok()
    } else {
        None
    }
}
