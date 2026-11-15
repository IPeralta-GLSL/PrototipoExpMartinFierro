//! Ciclo de la tarde a la noche estrellada.
//!
//! `progreso` va de 0 (atardecer) a 1 (noche cerrada):
//! - 0.00–0.55: el sol baja y se pone (la luz se apaga y se enrojece).
//! - 0.55–1.00: la misma luz direccional pasa a ser la luna (fría y tenue).
//! El cielo (shader), la niebla, el ambiente, el fogón, el farol, las
//! luciérnagas y el sonido acompañan la transición.

use godot::classes::{
    light_3d::Param, DirectionalLight3D, GpuParticles3D, Light3D, ShaderMaterial,
    StandardMaterial3D, WorldEnvironment,
};
use godot::prelude::*;

use crate::ambiente::AmbientePampa;
use crate::conversion::argumento_usuario;

#[inline]
fn suave(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn mezclar(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn mezclar_color(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        mezclar(a.r, b.r, t),
        mezclar(a.g, b.g, t),
        mezclar(a.b, b.b, t),
        1.0,
    )
}

#[derive(GodotClass)]
#[class(base = Node)]
pub struct CicloDia {
    #[export]
    sol: Option<Gd<DirectionalLight3D>>,
    #[export]
    entorno: Option<Gd<WorldEnvironment>>,
    #[export]
    cielo: Option<Gd<ShaderMaterial>>,
    #[export]
    luciernagas: Option<Gd<GpuParticles3D>>,
    #[export]
    ambiente: Option<Gd<AmbientePampa>>,
    /// Nodo Fogon: se le sube la energía de la luz y de las llamas.
    #[export]
    fogon: Option<Gd<Node3D>>,
    #[export]
    farol: Option<Gd<Light3D>>,
    /// Material emisivo de los carteles: de noche se atenúa su brillo.
    #[export]
    tablas: Option<Gd<StandardMaterial3D>>,
    /// Segundos que dura el anochecer completo.
    #[export]
    duracion: f32,
    /// 0 = empieza de tarde; 1 = empieza de noche (p. ej. el fortín).
    #[export]
    progreso_inicial: f32,
    /// Si es >= 0, el anochecer empieza solo tras esos segundos.
    #[export]
    automatico_tras: f32,

    progreso: f32,
    activo: bool,
    tiempo: f32,
    sol_inicial: Vector3,
    energia_sol: f32,
    color_sol: Color,
    energia_fogon: f32,
    llamas_fogon: f32,
    energia_farol: f32,
    base: Base<Node>,
}

#[godot_api]
impl INode for CicloDia {
    fn init(base: Base<Node>) -> Self {
        Self {
            sol: None,
            entorno: None,
            cielo: None,
            luciernagas: None,
            ambiente: None,
            fogon: None,
            farol: None,
            tablas: None,
            duracion: 70.0,
            progreso_inicial: 0.0,
            automatico_tras: -1.0,
            progreso: 0.0,
            activo: false,
            tiempo: 0.0,
            sol_inicial: Vector3::new(-9.0, 142.0, 0.0),
            energia_sol: 1.35,
            color_sol: Color::from_rgb(1.0, 0.71, 0.46),
            energia_fogon: 2.4,
            llamas_fogon: 2.2,
            energia_farol: 0.8,
            base,
        }
    }

    fn ready(&mut self) {
        if let Some(s) = &self.sol {
            self.sol_inicial = s.get_rotation_degrees();
            self.energia_sol = s.get_param(Param::ENERGY);
            self.color_sol = s.get_color();
        }
        if let Some(f) = &self.fogon {
            self.energia_fogon = f.get("energia_base").try_to::<f32>().unwrap_or(2.4);
            self.llamas_fogon = f.get("intensidad_llamas").try_to::<f32>().unwrap_or(2.2);
        }
        if let Some(f) = &self.farol {
            self.energia_farol = f.get_param(Param::ENERGY);
        }
        self.progreso = self.progreso_inicial.clamp(0.0, 1.0);
        // Para probar: -- --ciclo=0.7 arranca en ese punto del anochecer.
        if let Some(v) = argumento_usuario("ciclo").and_then(|v| v.parse::<f32>().ok()) {
            self.progreso = v.clamp(0.0, 1.0);
        }
        self.aplicar();
    }

    fn process(&mut self, delta: f64) {
        let dt = delta as f32;
        self.tiempo += dt;
        if !self.activo && self.automatico_tras >= 0.0 && self.tiempo >= self.automatico_tras {
            self.activo = true;
        }
        if self.activo && self.progreso < 1.0 {
            self.progreso = (self.progreso + dt / self.duracion.max(1.0)).min(1.0);
            self.aplicar();
        }
    }
}

#[godot_api]
impl CicloDia {
    /// Empieza a anochecer (lo llama el narrador).
    #[func]
    pub fn iniciar_anochecer(&mut self) {
        self.activo = true;
    }

    /// Salta a un punto del ciclo (0 = tarde, 1 = noche).
    #[func]
    pub fn fijar_progreso(&mut self, p: f32) {
        self.progreso = p.clamp(0.0, 1.0);
        self.aplicar();
    }

    #[func]
    pub fn progreso(&self) -> f32 {
        self.progreso
    }
}

impl CicloDia {
    fn aplicar(&mut self) {
        let p = self.progreso;
        let noche = suave(0.15, 0.85, p);
        // Resplandor del ocaso: crece al ponerse el sol y se apaga ya de noche.
        let crepusculo = suave(0.1, 0.4, p) * (1.0 - suave(0.55, 0.9, p));

        // --- Sol / luna ---
        if let Some(sol) = self.sol.as_mut() {
            if p < 0.55 {
                let u = p / 0.55;
                // El sol baja hasta quedar bajo el horizonte (ángulo positivo).
                let pitch = mezclar(self.sol_inicial.x, 5.0, suave(0.0, 1.0, u));
                sol.set_rotation_degrees(Vector3::new(pitch, self.sol_inicial.y, 0.0));
                sol.set_param(Param::ENERGY, self.energia_sol * (1.0 - suave(0.35, 0.95, u)));
                let rojo = Color::from_rgb(1.0, 0.42, 0.2);
                sol.set_color(mezclar_color(self.color_sol, rojo, suave(0.2, 0.9, u)));
            } else {
                let u = (p - 0.55) / 0.45;
                // Luna alta, adelante a la izquierda del fogón; luz fría y tenue.
                sol.set_rotation_degrees(Vector3::new(-37.0, -150.0, 0.0));
                sol.set_param(Param::ENERGY, 0.34 * suave(0.0, 0.6, u));
                sol.set_color(Color::from_rgb(0.62, 0.72, 1.0));
            }
        }

        // --- Cielo ---
        if let Some(cielo) = self.cielo.as_mut() {
            cielo.set_shader_parameter("noche", &noche.to_variant());
            cielo.set_shader_parameter("crepusculo", &crepusculo.to_variant());
            let y = self.sol_inicial.y.to_radians();
            // Dirección horizontal hacia el sol poniente (opuesta al haz de luz).
            let ocaso = Vector3::new(y.sin(), 0.0, y.cos());
            cielo.set_shader_parameter("dir_ocaso", &ocaso.to_variant());
        }

        // --- Ambiente, niebla y exposición ---
        if let Some(env) = self.entorno.as_ref().and_then(|e| e.get_environment()) {
            let mut env = env;
            env.set_ambient_light_energy(mezclar(1.1, 0.9, noche));
            let niebla_dia = Color::from_rgb(0.9, 0.67, 0.5);
            let niebla_noche = Color::from_rgb(0.05, 0.06, 0.1);
            env.set_fog_light_color(mezclar_color(niebla_dia, niebla_noche, noche));
            env.set_fog_sun_scatter(mezclar(0.4, 0.0, noche));
            env.set_fog_density(mezclar(0.0028, 0.0042, noche));
            env.set_fog_sky_affect(mezclar(0.12, 0.05, noche));
            env.set_tonemap_exposure(mezclar(1.0, 1.4, noche));
        }

        // --- Fuego, farol y luciérnagas ---
        if let Some(f) = self.fogon.as_mut() {
            f.set("energia_base", &(self.energia_fogon * mezclar(1.0, 1.5, noche)).to_variant());
            f.set("intensidad_llamas", &(self.llamas_fogon * mezclar(1.0, 1.25, noche)).to_variant());
        }
        if let Some(f) = self.farol.as_mut() {
            f.set_param(Param::ENERGY, self.energia_farol * mezclar(1.0, 2.0, noche));
        }
        if let Some(t) = self.tablas.as_mut() {
            t.set_emission_energy_multiplier(mezclar(1.0, 0.18, noche));
        }
        if let Some(l) = self.luciernagas.as_mut() {
            l.set_amount_ratio(mezclar(0.15, 1.0, noche));
        }

        // --- Sonido: calma el viento, suben los grillos ---
        if let Some(a) = self.ambiente.as_mut() {
            a.bind_mut()
                .fijar_volumenes(mezclar(0.45, 0.22, noche), mezclar(0.07, 0.16, noche));
        }
    }
}
