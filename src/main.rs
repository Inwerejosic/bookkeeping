// use actix_web::{App, HttpResponse, HttpServer, Responder, web};
// use chrono::{DateTime, Utc};
// use serde::{Deserialize, Serialize};
// use std::fs::File;
// use std::io::{Read, Write};
// use std::path::Path;
// use std::sync::Mutex;
// use uuid::Uuid;

// #[derive(Serialize, Deserialize, Clone)]
// struct Transaction {
//     id: String,
//     user: String,
//     item: String,
//     amount: f32,
//     date: DateTime<Utc>,
// }

// #[derive(Serialize, Deserialize)]
// struct NewTransaction {
//     user: String,
//     item: String,
//     amount: f32,
// }

// struct AppState {
//     transactions: Mutex<Vec<Transaction>>,
//     file_path: String,
// }

// //
// // FILE STORAGE
// //

// async fn save_transactions(state: &web::Data<AppState>) -> std::io::Result<()> {
//     let txs = state.transactions.lock().unwrap();
//     let json = serde_json::to_string_pretty(&*txs)?;
//     let mut file = File::create(&state.file_path)?;
//     file.write_all(json.as_bytes())?;
//     Ok(())
// }

// async fn load_transactions(file_path: &str) -> Vec<Transaction> {
//     if Path::new(file_path).exists() {
//         let mut file = File::open(file_path).expect("Cannot open file");
//         let mut data = String::new();
//         file.read_to_string(&mut data).unwrap();
//         serde_json::from_str(&data).unwrap_or_default()
//     } else {
//         Vec::new()
//     }
// }

// //
// // ROUTES
// //

// async fn add_transaction(
//     body: web::Json<NewTransaction>,
//     state: web::Data<AppState>,
// ) -> impl Responder {
//     let mut txs = state.transactions.lock().unwrap();

//     let tx = Transaction {
//         id: Uuid::new_v4().to_string(),
//         user: body.user.clone(),
//         item: body.item.clone(),
//         amount: body.amount,
//         date: Utc::now(),
//     };

//     txs.push(tx.clone());
//     save_transactions(&state).await.unwrap();

//     HttpResponse::Ok().json(tx)
// }

// async fn list_transactions(state: web::Data<AppState>) -> impl Responder {
//     let txs = state.transactions.lock().unwrap();
//     HttpResponse::Ok().json(&*txs)
// }

// async fn delete_transaction(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
//     let id = path.into_inner();
//     let mut txs = state.transactions.lock().unwrap();

//     txs.retain(|t| t.id != id);

//     save_transactions(&state).await.unwrap();
//     HttpResponse::Ok().json("Deleted")
// }

// #[derive(Deserialize)]
// struct UpdateTx {
//     item: Option<String>,
//     amount: Option<f32>,
// }

// async fn update_transaction(
//     state: web::Data<AppState>,
//     path: web::Path<String>,
//     body: web::Json<UpdateTx>,
// ) -> impl Responder {
//     let id = path.into_inner();
//     let mut txs = state.transactions.lock().unwrap();

//     if let Some(tx) = txs.iter_mut().find(|t| t.id == id) {
//         if let Some(item) = &body.item {
//             tx.item = item.clone();
//         }
//         if let Some(amount) = body.amount {
//             tx.amount = amount;
//         }
//     }

//     save_transactions(&state).await.unwrap();
//     HttpResponse::Ok().json("Updated")
// }

// async fn report_for_user(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
//     let username = path.into_inner();
//     let txs = state.transactions.lock().unwrap();

//     let total: f32 = txs
//         .iter()
//         .filter(|t| t.user == username)
//         .map(|t| t.amount)
//         .sum();

//     HttpResponse::Ok().json(serde_json::json!({
//         "user": username,
//         "total_spent": total,
//         "entries": txs.iter().filter(|t| t.user == username).cloned().collect::<Vec<_>>()
//     }))
// }

// //
// // MAIN
// //

// #[actix_web::main]
// async fn main() -> std::io::Result<()> {
//     let file_path = "transactions.json".to_string();
//     let existing = load_transactions(&file_path).await;

//     let state = web::Data::new(AppState {
//         transactions: Mutex::new(existing),
//         file_path: file_path.clone(),
//     });

//     println!("Bookkeeping API running at: http://localhost:3000");

//     HttpServer::new(move || {
//         App::new()
//             .app_data(state.clone())
//             .route("/transactions", web::post().to(add_transaction))
//             .route("/transactions", web::get().to(list_transactions))
//             .route("/transactions/{id}", web::delete().to(delete_transaction))
//             .route("/transactions/{id}", web::put().to(update_transaction))
//             .route("/report/{user}", web::get().to(report_for_user))
//     })
//     .bind("127.0.0.1:3000")?
//     .run()
//     .await
// }

use actix_web::{App, HttpResponse, HttpServer, Responder, delete, get, post, put, web};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;
use uuid::Uuid;

const STORAGE_FILE: &str = "transactions.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    pub id: Uuid,
    pub user: String,
    pub item: String,
    pub amount: f64,
    /// UNIX timestamp (seconds since epoch)
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTransaction {
    pub user: String,
    pub item: String,
    pub amount: f64,
    /// optional: if omitted server will fill current timestamp
    pub timestamp: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTransaction {
    pub user: Option<String>,
    pub item: Option<String>,
    pub amount: Option<f64>,
    pub timestamp: Option<u64>,
}

#[derive(Clone)]
struct AppState {
    /// async RwLock protects the vector; Arc-wrap via web::Data
    transactions: Arc<RwLock<Vec<Transaction>>>,
    file_path: String,
}

impl AppState {
    async fn persist(&self) -> std::io::Result<()> {
        // Take a snapshot of the data under a read lock, then write the file asynchronously.
        let snapshot = {
            let read_guard = self.transactions.read().await;
            serde_json::to_vec_pretty(&*read_guard)?
        };

        // Write to temp file then rename for atomicity
        let tmp_path = format!("{}.tmp", &self.file_path);
        fs::write(&tmp_path, snapshot).await?;
        fs::rename(&tmp_path, &self.file_path).await?;
        Ok(())
    }

    async fn load(file_path: impl Into<String>) -> std::io::Result<Vec<Transaction>> {
        let file_path = file_path.into();
        if Path::new(&file_path).exists() {
            let data = fs::read(&file_path).await?;
            let txs: Vec<Transaction> = serde_json::from_slice(&data).unwrap_or_default();
            Ok(txs)
        } else {
            Ok(Vec::new())
        }
    }
}

#[post("/transactions")]
async fn create_transaction(
    state: web::Data<AppState>,
    payload: web::Json<CreateTransaction>,
) -> impl Responder {
    // Basic validation
    if payload.user.trim().is_empty() || payload.item.trim().is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "user and item must be non-empty strings"
        }));
    }
    if !payload.amount.is_finite() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "amount must be a finite number"
        }));
    }

    let ts = payload.timestamp.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });

    let tx = Transaction {
        id: Uuid::new_v4(),
        user: payload.user.trim().to_string(),
        item: payload.item.trim().to_string(),
        amount: payload.amount,
        timestamp: ts,
    };

    {
        // acquire write lock, mutate, then release before any await
        let mut write_guard = state.transactions.write().await;
        write_guard.push(tx.clone());
    } // lock released here

    // persist asynchronously
    if let Err(e) = state.persist().await {
        eprintln!("Failed to persist transactions: {}", e);
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "failed to save transaction"
        }));
    }

    HttpResponse::Created().json(tx)
}

#[get("/transactions")]
async fn list_transactions(state: web::Data<AppState>) -> impl Responder {
    let read_guard = state.transactions.read().await;
    HttpResponse::Ok().json(read_guard.clone())
}

#[get("/transactions/{id}")]
async fn get_transaction(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let id_str = path.into_inner();
    let id = match Uuid::parse_str(&id_str) {
        Ok(u) => u,
        Err(_) => {
            return HttpResponse::BadRequest().json(serde_json::json!({"error":"invalid uuid"}));
        }
    };

    let read_guard = state.transactions.read().await;
    if let Some(tx) = read_guard.iter().find(|t| t.id == id) {
        HttpResponse::Ok().json(tx.clone())
    } else {
        HttpResponse::NotFound().json(serde_json::json!({"error":"not found"}))
    }
}

#[put("/transactions/{id}")]
async fn update_transaction(
    state: web::Data<AppState>,
    path: web::Path<String>,
    payload: web::Json<UpdateTransaction>,
) -> impl Responder {
    let id_str = path.into_inner();
    let id = match Uuid::parse_str(&id_str) {
        Ok(u) => u,
        Err(_) => {
            return HttpResponse::BadRequest().json(serde_json::json!({"error":"invalid uuid"}));
        }
    };

    {
        let mut write_guard = state.transactions.write().await;
        if let Some(tx) = write_guard.iter_mut().find(|t| t.id == id) {
            if let Some(user) = &payload.user {
                if user.trim().is_empty() {
                    return HttpResponse::BadRequest()
                        .json(serde_json::json!({"error":"user cannot be empty"}));
                }
                tx.user = user.trim().to_string();
            }
            if let Some(item) = &payload.item {
                if item.trim().is_empty() {
                    return HttpResponse::BadRequest()
                        .json(serde_json::json!({"error":"item cannot be empty"}));
                }
                tx.item = item.trim().to_string();
            }
            if let Some(amount) = payload.amount {
                if !amount.is_finite() {
                    return HttpResponse::BadRequest()
                        .json(serde_json::json!({"error":"amount must be finite"}));
                }
                tx.amount = amount;
            }
            if let Some(ts) = payload.timestamp {
                tx.timestamp = ts;
            }
        } else {
            return HttpResponse::NotFound().json(serde_json::json!({"error":"not found"}));
        }
    } // lock released before await

    if let Err(e) = state.persist().await {
        eprintln!("Failed to persist after update: {}", e);
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error":"failed to save changes"}));
    }

    // return the updated item
    let read_guard = state.transactions.read().await;
    let updated = read_guard.iter().find(|t| t.id == id).cloned().unwrap();
    HttpResponse::Ok().json(updated)
}

#[delete("/transactions/{id}")]
async fn delete_transaction(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let id_str = path.into_inner();
    let id = match Uuid::parse_str(&id_str) {
        Ok(u) => u,
        Err(_) => {
            return HttpResponse::BadRequest().json(serde_json::json!({"error":"invalid uuid"}));
        }
    };

    {
        let mut write_guard = state.transactions.write().await;
        let initial_len = write_guard.len();
        write_guard.retain(|t| t.id != id);
        if write_guard.len() == initial_len {
            return HttpResponse::NotFound().json(serde_json::json!({"error":"not found"}));
        }
    }

    if let Err(e) = state.persist().await {
        eprintln!("Failed to persist after delete: {}", e);
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error":"failed to persist delete"}));
    }

    HttpResponse::NoContent().finish()
}

#[get("/users/{user}/summary")]
async fn user_summary(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let user = path.into_inner();
    let read_guard = state.transactions.read().await;
    let user_txs: Vec<Transaction> = read_guard
        .iter()
        .filter(|t| t.user == user)
        .cloned()
        .collect();
    let total: f64 = user_txs.iter().map(|t| t.amount).sum();
    let count = user_txs.len();
    HttpResponse::Ok().json(serde_json::json!({
        "user": user,
        "count": count,
        "total_amount": total,
        "transactions": user_txs
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load existing transactions from disk
    let initial = AppState::load(STORAGE_FILE).await.unwrap_or_default();

    let state = AppState {
        transactions: Arc::new(RwLock::new(initial)),
        file_path: STORAGE_FILE.to_string(),
    };

    let shared = web::Data::new(state);

    println!("Server running at http://127.0.0.1:3000");

    HttpServer::new(move || {
        App::new()
            .app_data(shared.clone())
            .service(create_transaction)
            .service(list_transactions)
            .service(get_transaction)
            .service(update_transaction)
            .service(delete_transaction)
            .service(user_summary)
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
