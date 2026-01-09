use warp::Filter;
use tracing::info;

mod state;
pub use state::{SharedNetworkState, new_shared_network_state};

/// Inicia el servidor HTTP local para comunicación entre nodos
/// 
/// # Descripción
/// Levanta un servidor HTTP en 127.0.0.1:8080 con los siguientes endpoints:
/// - GET /: Devuelve la página HTML de la UI
/// - GET /status: Devuelve {"estado": "activo"}
/// - GET /network: Devuelve un snapshot de red (peers, bootstrap peers, etc.)
/// 
/// # Ejemplo
/// ```bash
/// curl http://127.0.0.1:8080/status
/// # Respuesta: {"estado":"activo"}
/// ```
pub async fn iniciar_api_local(network_state: SharedNetworkState) {
    info!("Iniciando API local en 127.0.0.1:8080");

    // Definir el endpoint para la UI (GET /)
    let ui_route = warp::path::end()
        .and(warp::get())
        .map(|| {
            warp::reply::html(include_str!("../../static/index.html"))
        });

    // Definir el endpoint /status
    let status_route = warp::path("status")
        .and(warp::get())
        .map(|| {
            warp::reply::json(&serde_json::json!({
                "estado": "activo"
            }))
        });

    // Definir el endpoint /network (snapshot)
    let with_state = warp::any().map(move || network_state.clone());
    let network_route = warp::path("network")
        .and(warp::get())
        .and(with_state)
        .and_then(|state: SharedNetworkState| async move {
            let snapshot = state.read().await.clone();
            Ok::<_, std::convert::Infallible>(warp::reply::json(&snapshot))
        });

    // Combinar todas las rutas
    let routes = ui_route.or(status_route).or(network_route);

    info!("API local lista. Endpoints disponibles:");
    info!("  GET http://127.0.0.1:8080/");
    info!("  GET http://127.0.0.1:8080/status");
    info!("  GET http://127.0.0.1:8080/network");

    // Iniciar el servidor
    warp::serve(routes)
        .run(([127, 0, 0, 1], 8080))
        .await;
}
