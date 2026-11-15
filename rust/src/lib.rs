//! Martín Fierro VR — extensión de Godot escrita en Rust (gdext).
//!
//! Clases que expone a Godot:
//! - `TerrenoPampa`: suelo procedural de la pampa (relieve, color, alturas).
//! - `PastoPampa`: pasto y cortaderas con MultiMesh por parcelas.
//! - `AmbientePampa`: viento y grillos sintetizados; mueve el pasto.
//! - `Fogon`: luz titilante y crepitar 3D del fuego.
//! - `GuitarraCriolla`: guitarra que se rasguea con la mano (Karplus-Strong).
//! - `JugadorXR`: origen XR con locomoción, modo escritorio y capturas.
//! - `ModeloIA` / `DispersorModelos`: modelos generados con Hunyuan3D-2.

use godot::prelude::*;

pub mod ambiente;
pub mod azar;
pub mod conversion;
pub mod geometria;
pub mod guitarra;
pub mod jugador;
pub mod modelos;
pub mod pasto;
pub mod ruido;
pub mod sintesis;
pub mod terreno;

struct MartinFierro;

#[gdextension]
unsafe impl ExtensionLibrary for MartinFierro {}
