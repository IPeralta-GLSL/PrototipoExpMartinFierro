//! Modelos generados con IA local (Hunyuan3D-2) en `res://modelos/<nombre>.glb`.
//!
//! - `ModeloIA`: un objeto. Si el GLB existe lo instancia y oculta el hijo
//!   "Marcador" (geometría provisoria); si no existe, deja el marcador.
//! - `DispersorModelos`: siembra varias copias sobre el terreno (cardos,
//!   vacas, ñandúes...) evitando la tierra pisada.
//!
//! Ambas clases son `tool`: los modelos también se ven en el editor, como
//! hijos internos que no se guardan en la escena.

use std::f32::consts::TAU;

use godot::classes::{
    geometry_instance_3d::ShadowCastingSetting, Engine, GeometryInstance3D, Node, PackedScene,
    ResourceLoader,
};
use godot::prelude::*;

use crate::azar::Azar;
use crate::terreno::TerrenoPampa;

fn ruta_modelo(nombre: &str) -> String {
    format!("res://modelos/{nombre}.glb")
}

pub fn modelo_existe(nombre: &str) -> bool {
    !nombre.is_empty() && ResourceLoader::singleton().exists(&ruta_modelo(nombre))
}

pub fn instanciar_modelo(nombre: &str) -> Option<Gd<Node3D>> {
    if !modelo_existe(nombre) {
        return None;
    }
    let escena = try_load::<PackedScene>(&ruta_modelo(nombre)).ok()?;
    escena.instantiate()?.try_cast::<Node3D>().ok()
}

/// Sombras y distancia de corte para todas las geometrías de un modelo.
fn configurar_geometria(n: &Gd<Node>, sombras: bool, distancia: f32) {
    if let Ok(mut g) = n.clone().try_cast::<GeometryInstance3D>() {
        g.set_cast_shadows_setting(if sombras {
            ShadowCastingSetting::ON
        } else {
            ShadowCastingSetting::OFF
        });
        if distancia > 0.0 {
            g.set_visibility_range_end(distancia);
        }
    }
    for hijo in n.get_children().iter_shared() {
        configurar_geometria(&hijo, sombras, distancia);
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(tool, base = Node3D)]
pub struct ModeloIA {
    /// Nombre del GLB en res://modelos (sin extensión).
    #[export]
    modelo: GString,
    #[export]
    sombras: bool,
    /// Distancia a la que deja de dibujarse (0 = siempre).
    #[export]
    distancia_visible: f32,
    /// Respiración (animales): amplitud de escala, p. ej. 0.008. 0 = quieto.
    #[export]
    respirar: f32,
    /// Período de la respiración, en segundos.
    #[export]
    periodo: f32,

    instancia: Option<Gd<Node3D>>,
    t: f64,
    espera: f64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for ModeloIA {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            modelo: GString::new(),
            sombras: true,
            distancia_visible: 0.0,
            respirar: 0.0,
            periodo: 3.5,
            instancia: None,
            t: 0.0,
            espera: 0.0,
            base,
        }
    }

    fn ready(&mut self) {
        self.cargar();
        // Desfase distinto por instancia para que no respiren todos juntos.
        self.t = (self.base().instance_id().to_i64().unsigned_abs() % 997) as f64 * 0.37;
        let editor = Engine::singleton().is_editor_hint();
        // En el editor se sigue procesando mientras el GLB no exista, para que
        // aparezca solo cuando la IA termine de generarlo.
        let procesar = (self.respirar > 0.0 && !editor) || (editor && self.instancia.is_none());
        self.base_mut().set_process(procesar);
    }

    fn process(&mut self, delta: f64) {
        self.t += delta;
        if self.instancia.is_none() {
            self.espera += delta;
            if self.espera > 2.0 {
                self.espera = 0.0;
                if self.cargar() && Engine::singleton().is_editor_hint() {
                    self.base_mut().set_process(false);
                }
            }
            return;
        }
        let Some(m) = self.instancia.as_mut() else {
            return;
        };
        let fase = (self.t * std::f64::consts::TAU / self.periodo.max(0.5) as f64).sin() as f32;
        let a = self.respirar;
        m.set_scale(Vector3::new(1.0 + a * 0.6 * fase, 1.0 + a * fase, 1.0 + a * 0.6 * fase));
    }
}

#[godot_api]
impl ModeloIA {
    /// (Re)carga el modelo. Devuelve false si el GLB todavía no existe.
    #[func]
    pub fn cargar(&mut self) -> bool {
        if let Some(mut vieja) = self.instancia.take() {
            vieja.queue_free();
        }
        let nombre = self.modelo.to_string();
        let Some(mut n) = instanciar_modelo(&nombre) else {
            return false;
        };
        configurar_geometria(&n.clone().upcast::<Node>(), self.sombras, self.distancia_visible);
        n.set_name("ModeloGenerado");
        if let Some(m) = self.base().get_node_or_null("Marcador") {
            if let Ok(mut m3) = m.try_cast::<Node3D>() {
                m3.hide();
            }
        }
        self.base_mut().add_child(&n);
        self.instancia = Some(n);
        true
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(tool, base = Node3D)]
pub struct DispersorModelos {
    #[export]
    terreno: Option<Gd<TerrenoPampa>>,
    #[export]
    modelo: GString,
    #[export]
    cantidad: i32,
    #[export]
    radio_min: f32,
    #[export]
    radio_max: f32,
    /// Sector en grados: 0 = hacia -Z (adelante del jugador), positivo = a la derecha.
    #[export]
    angulo_min: f32,
    #[export]
    angulo_max: f32,
    #[export]
    escala_min: f32,
    #[export]
    escala_max: f32,
    /// Distancia mínima entre copias (m).
    #[export]
    separacion: f32,
    #[export]
    semilla: i64,
    #[export]
    sombras: bool,
    #[export]
    distancia_visible: f32,
    /// No sembrar sobre la tierra pisada (fogón, senderos).
    #[export]
    evitar_tierra: bool,
    /// Respiración de cada copia (animales). 0 = quietos.
    #[export]
    respirar: f32,
    #[export]
    periodo: f32,

    instancias: Vec<Gd<Node3D>>,
    escalas: Vec<f32>,
    t: f64,
    espera: f64,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for DispersorModelos {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            terreno: None,
            modelo: GString::new(),
            cantidad: 8,
            radio_min: 6.0,
            radio_max: 30.0,
            angulo_min: 0.0,
            angulo_max: 360.0,
            escala_min: 0.85,
            escala_max: 1.15,
            separacion: 2.0,
            semilla: 1,
            sombras: true,
            distancia_visible: 0.0,
            evitar_tierra: true,
            respirar: 0.0,
            periodo: 3.5,
            instancias: Vec::new(),
            escalas: Vec::new(),
            t: 0.0,
            espera: 0.0,
            base,
        }
    }

    fn ready(&mut self) {
        self.regenerar();
        let editor = Engine::singleton().is_editor_hint();
        let procesar = (self.respirar > 0.0 && !editor) || (editor && self.instancias.is_empty());
        self.base_mut().set_process(procesar);
    }

    fn process(&mut self, delta: f64) {
        self.t += delta;
        if self.instancias.is_empty() {
            // Todavía no existe el GLB: reintentar cada 2 s (útil en el editor).
            self.espera += delta;
            if self.espera > 2.0 {
                self.espera = 0.0;
                self.regenerar();
                if !self.instancias.is_empty() && Engine::singleton().is_editor_hint() {
                    self.base_mut().set_process(false);
                }
            }
            return;
        }
        let w = std::f64::consts::TAU / self.periodo.max(0.5) as f64;
        let a = self.respirar;
        for (i, (n, s)) in self.instancias.iter_mut().zip(self.escalas.iter()).enumerate() {
            let fase = (self.t * w + i as f64 * 1.7).sin() as f32;
            n.set_scale(Vector3::new(
                s * (1.0 + a * 0.6 * fase),
                s * (1.0 + a * fase),
                s * (1.0 + a * 0.6 * fase),
            ));
        }
    }
}

#[godot_api]
impl DispersorModelos {
    #[func]
    pub fn regenerar(&mut self) {
        for mut n in self.instancias.drain(..) {
            n.queue_free();
        }
        self.escalas.clear();
        let nombre = self.modelo.to_string();
        if !modelo_existe(&nombre) {
            return;
        }
        let Some(terreno) = self.terreno.clone() else {
            godot_warn!("DispersorModelos '{nombre}': falta asignar 'terreno'.");
            return;
        };

        // 1) Elegir lugares (sin tocar el árbol de escena todavía).
        let mut az = Azar::new(self.semilla as u64);
        let mut lugares: Vec<(Vector3, f32, f32)> = Vec::new();
        {
            let t = terreno.bind();
            let objetivo = self.cantidad.max(0) as usize;
            let (r0, r1) = (self.radio_min.max(0.0), self.radio_max.max(self.radio_min));
            for _ in 0..(objetivo * 40 + 10) {
                if lugares.len() >= objetivo {
                    break;
                }
                let r = (r0 * r0 + az.f32() * (r1 * r1 - r0 * r0)).sqrt();
                let ang = az.rango(self.angulo_min, self.angulo_max).to_radians();
                let (x, z) = (r * ang.sin(), -r * ang.cos());
                if self.evitar_tierra && t.cobertura(x, z) < 0.85 {
                    continue;
                }
                if lugares
                    .iter()
                    .any(|(p, _, _)| Vector2::new(p.x - x, p.z - z).length() < self.separacion)
                {
                    continue;
                }
                let y = t.altura(x, z);
                let giro = az.rango(0.0, TAU);
                let escala = az.rango(self.escala_min, self.escala_max.max(self.escala_min));
                lugares.push((Vector3::new(x, y, z), giro, escala));
            }
        }

        // 2) Instanciar.
        for (pos, giro, escala) in lugares {
            let Some(mut n) = instanciar_modelo(&nombre) else {
                break;
            };
            configurar_geometria(&n.clone().upcast::<Node>(), self.sombras, self.distancia_visible);
            // Node3D guarda rotación y escala por separado: la respiración puede
            // animar la escala sin perder el giro.
            n.set_position(pos);
            n.set_rotation(Vector3::new(0.0, giro, 0.0));
            n.set_scale(Vector3::splat(escala));
            self.base_mut().add_child(&n);
            self.instancias.push(n);
            self.escalas.push(escala);
        }
    }
}
