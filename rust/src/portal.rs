//! Portal del fogón: "mirá el fuego, paisano". La experiencia es lineal y por
//! tiempo: cuando el narrador termina su última línea, el fuego crece, las
//! brasas se arremolinan y la vista se funde en naranja y negro hasta pasar
//! al otro escenario. No espera ninguna acción del jugador.

use godot::classes::{light_3d::Param, Engine, GpuParticles3D, Light3D};
use godot::prelude::*;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::conversion::argumento_usuario;

/// `--portal` (prueba) se usa una sola vez por ejecución, para no rebotar entre escenas.
static PORTAL_DE_PRUEBA_USADO: AtomicBool = AtomicBool::new(false);

#[inline]
fn suave(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct PortalFogon {
    /// Escena a la que lleva el fuego.
    #[export]
    escena_destino: GString,
    /// Nodo Fogon (luz y llamas).
    #[export]
    fogon: Option<Gd<Node3D>>,
    #[export]
    brasas: Option<Gd<GpuParticles3D>>,
    /// Sin fuego (la pulpería): una luz que crece durante el fundido (el farol).
    #[export]
    luz: Option<Gd<Light3D>>,
    /// Segundos que dura el fundido del fuego.
    #[export]
    duracion: f32,
    /// Red de seguridad: si el narrador no lo dispara, el portal se abre solo
    /// a los N segundos de empezar la escena (0 = desactivado).
    #[export]
    tiempo_maximo: f32,

    /// Cuenta regresiva hasta empezar el fundido (< 0 = sin programar).
    cuenta: f32,
    transicion: f32,
    tiempo: f32,
    energia0: f32,
    llamas0: f32,
    luz0: f32,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for PortalFogon {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            escena_destino: GString::new(),
            fogon: None,
            brasas: None,
            luz: None,
            duracion: 4.0,
            tiempo_maximo: 0.0,
            cuenta: -1.0,
            transicion: -1.0,
            tiempo: 0.0,
            energia0: 2.4,
            llamas0: 2.2,
            luz0: 1.0,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            self.base_mut().set_process(false);
            return;
        }
        // Para probar: -- --portal pasa de escena enseguida.
        if argumento_usuario("portal").is_some()
            && !PORTAL_DE_PRUEBA_USADO.swap(true, Ordering::Relaxed)
        {
            self.activar_en(1.0);
        }
    }

    fn process(&mut self, delta: f64) {
        let dt = delta as f32;
        self.tiempo += dt;

        if self.transicion < 0.0 {
            if self.cuenta < 0.0 && self.tiempo_maximo > 0.0 && self.tiempo >= self.tiempo_maximo
            {
                godot_print!("Portal del fogón: tiempo máximo de la escena cumplido");
                self.cuenta = 0.0;
            }
            if self.cuenta >= 0.0 {
                self.cuenta -= dt;
                if self.cuenta <= 0.0 {
                    self.iniciar_transicion();
                }
            }
            return;
        }

        self.transicion += dt;
        let u = (self.transicion / self.duracion.max(0.5)).min(1.0);
        let (e0, l0) = (self.energia0, self.llamas0);
        if let Some(f) = self.fogon.as_mut() {
            f.set("energia_base", &(e0 * (1.0 + 4.0 * u)).to_variant());
            f.set("intensidad_llamas", &(l0 * (1.0 + 1.8 * u)).to_variant());
        }
        let l0 = self.luz0;
        if let Some(l) = self.luz.as_mut() {
            l.set_param(Param::ENERGY, l0 * (1.0 + 5.0 * u * u));
        }
        if let Some(b) = self.brasas.as_mut() {
            b.set_amount_ratio(1.0);
            b.set_speed_scale(1.0 + 3.0 * u as f64);
        }
        // Velo: primero el resplandor naranja del fuego, después negro.
        let naranja = Color::from_rgb(1.0, 0.45, 0.12);
        let negro = Color::from_rgb(0.0, 0.0, 0.0);
        let mezcla = suave(0.55, 0.95, u);
        let mut c = naranja.lerp(negro, mezcla as f64);
        c.a = suave(0.15, 0.6, u);
        self.fijar_velo(c);

        if u >= 1.0 {
            self.base_mut().set_process(false);
            let destino = self.escena_destino.clone();
            if !destino.is_empty() {
                let r = self.base().get_tree().change_scene_to_file(&destino);
                godot_print!("Portal del fogón -> {destino} ({r:?})");
            }
        }
    }
}

#[godot_api]
impl PortalFogon {
    /// Programa el fundido del fuego para dentro de `segundos`
    /// (lo llama el narrador al decir su última línea).
    #[func]
    pub fn activar_en(&mut self, segundos: f32) {
        if self.transicion >= 0.0 {
            return;
        }
        let s = segundos.max(0.0);
        // Si ya estaba programado, gana el que llegue antes.
        self.cuenta = if self.cuenta >= 0.0 { self.cuenta.min(s) } else { s };
    }

    /// Empieza el fundido ya mismo.
    #[func]
    pub fn iniciar_transicion(&mut self) {
        if self.transicion >= 0.0 {
            return;
        }
        if let Some(f) = &self.fogon {
            // Parte de la intensidad actual (el ciclo de la noche ya la subió).
            self.energia0 = f.get("energia_base").try_to::<f32>().unwrap_or(2.4);
            self.llamas0 = f.get("intensidad_llamas").try_to::<f32>().unwrap_or(2.2);
        }
        if let Some(l) = &self.luz {
            self.luz0 = l.get_param(Param::ENERGY);
        }
        self.cuenta = -1.0;
        self.transicion = 0.0;
        godot_print!("Portal del fogón: comienza el fundido");
    }
}

impl PortalFogon {
    /// Pide al jugador (grupo "jugador") que tiña su velo de cámara.
    fn fijar_velo(&mut self, color: Color) {
        let arbol = self.base().get_tree();
        for mut j in arbol.get_nodes_in_group("jugador").iter_shared() {
            j.call("fijar_velo", &[color.to_variant()]);
        }
    }
}
