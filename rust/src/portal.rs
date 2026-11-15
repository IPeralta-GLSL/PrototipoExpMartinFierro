//! Portal del fogón: "mirá el fuego, paisano". Al acercarse a las llamas,
//! el fuego crece, las brasas se arremolinan y la vista se funde en naranja y
//! negro hasta pasar al otro escenario (el recuerdo del narrador).

use godot::classes::{
    base_material_3d::BillboardMode, Engine, GpuParticles3D, Input, Label3D, SystemFont,
};
use godot::global::Key;
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
    /// Distancia horizontal (m) desde la que el fuego "te lleva".
    #[export]
    radio: f32,
    #[export]
    duracion: f32,
    /// Texto de la pista que aparece sobre el fuego.
    #[export]
    texto_pista: GString,
    #[export]
    activo_al_inicio: bool,

    activo: bool,
    transicion: f32,
    t: f32,
    tecla_previa: bool,
    energia0: f32,
    llamas0: f32,
    pista: Option<Gd<Label3D>>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for PortalFogon {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            escena_destino: GString::new(),
            fogon: None,
            brasas: None,
            radio: 1.7,
            duracion: 4.0,
            texto_pista: GString::from("Acercate al fuego y mirá las llamas"),
            activo_al_inicio: false,
            activo: false,
            transicion: -1.0,
            t: 0.0,
            tecla_previa: false,
            energia0: 2.4,
            llamas0: 2.2,
            pista: None,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let mut fuente = SystemFont::new_gd();
        fuente.set_font_names(&PackedStringArray::from(&[
            GString::from("Georgia"),
            GString::from("Times New Roman"),
            GString::from("serif"),
        ]));
        let mut pista = Label3D::new_alloc();
        pista.set_name("Pista");
        pista.set_text(&self.texto_pista);
        pista.set_font(&fuente);
        pista.set_font_size(34);
        pista.set_pixel_size(0.0022);
        pista.set_outline_size(10);
        pista.set_billboard_mode(BillboardMode::FIXED_Y);
        pista.set_modulate(Color::from_rgba(1.0, 0.82, 0.55, 0.0));
        pista.set_outline_modulate(Color::from_rgba(0.1, 0.04, 0.0, 0.0));
        pista.set_position(Vector3::new(0.0, 1.45, 0.0));
        self.base_mut().add_child(&pista);
        self.pista = Some(pista);
        // Para probar: -- --portal lo habilita desde el comienzo.
        let prueba = argumento_usuario("portal").is_some()
            && !PORTAL_DE_PRUEBA_USADO.swap(true, Ordering::Relaxed);
        if self.activo_al_inicio || prueba {
            self.activar();
        }
    }

    fn process(&mut self, delta: f64) {
        if !self.activo {
            return;
        }
        let dt = delta as f32;
        self.t += dt;

        if self.transicion < 0.0 {
            // Pista que late suavemente sobre el fuego.
            let alfa = (self.t / 1.5).min(1.0) * (0.75 + 0.25 * (self.t * 2.4).sin());
            self.fijar_alfa_pista(alfa);
            let tecla = Input::singleton().is_key_pressed(Key::E);
            let tecla_nueva = tecla && !self.tecla_previa;
            self.tecla_previa = tecla;
            if tecla_nueva || self.jugador_cerca() {
                self.iniciar_transicion();
            }
            return;
        }

        self.transicion += dt;
        let u = (self.transicion / self.duracion.max(0.5)).min(1.0);
        self.fijar_alfa_pista((1.0 - u * 3.0).max(0.0));
        let (e0, l0) = (self.energia0, self.llamas0);
        if let Some(f) = self.fogon.as_mut() {
            f.set("energia_base", &(e0 * (1.0 + 4.0 * u)).to_variant());
            f.set("intensidad_llamas", &(l0 * (1.0 + 1.8 * u)).to_variant());
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
            self.activo = false;
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
    /// Habilita el portal (lo llama el narrador al final del relato).
    #[func]
    pub fn activar(&mut self) {
        if self.activo {
            return;
        }
        self.activo = true;
        self.t = 0.0;
        if let Some(f) = &self.fogon {
            self.energia0 = f.get("energia_base").try_to::<f32>().unwrap_or(2.4);
            self.llamas0 = f.get("intensidad_llamas").try_to::<f32>().unwrap_or(2.2);
        }
    }

    #[func]
    pub fn iniciar_transicion(&mut self) {
        if self.transicion < 0.0 {
            self.activo = true;
            self.transicion = 0.0;
        }
    }
}

impl PortalFogon {
    fn jugador_cerca(&self) -> bool {
        let Some(cam) = self.base().get_viewport().and_then(|v| v.get_camera_3d()) else {
            return false;
        };
        let p = cam.get_global_position();
        let c = self.base().get_global_position();
        Vector2::new(p.x - c.x, p.z - c.z).length() < self.radio
    }

    fn fijar_alfa_pista(&mut self, alfa: f32) {
        if let Some(p) = self.pista.as_mut() {
            let mut c = p.get_modulate();
            c.a = alfa;
            p.set_modulate(c);
            let mut o = p.get_outline_modulate();
            o.a = alfa * 0.9;
            p.set_outline_modulate(o);
        }
    }

    /// Pide al jugador (grupo "jugador") que tiña su velo de cámara.
    fn fijar_velo(&mut self, color: Color) {
        let arbol = self.base().get_tree();
        for mut j in arbol.get_nodes_in_group("jugador").iter_shared() {
            j.call("fijar_velo", &[color.to_variant()]);
        }
    }
}
