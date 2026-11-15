//! "Aquí me pongo a cantar / al compás de la vigüela": la guitarra del
//! fogón se rasguea pasando la mano rápido por la boca (o con el gatillo),
//! con síntesis Karplus-Strong en Rust.

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, AudioStreamPlayer3D, Engine, Input, XrController3D,
};
use godot::global::Key;
use godot::prelude::*;

use crate::ambiente::GeneradorAudio;
use crate::sintesis::{Guitarra, ACORDE_LA_MENOR, ACORDE_MI_MENOR, ACORDE_SI_SEPTIMA};

/// Secuencia de milonga: Mi menor y Si séptima, con un paso por La menor.
const SECUENCIA: [&[f32; 6]; 8] = [
    &ACORDE_MI_MENOR,
    &ACORDE_MI_MENOR,
    &ACORDE_SI_SEPTIMA,
    &ACORDE_SI_SEPTIMA,
    &ACORDE_MI_MENOR,
    &ACORDE_LA_MENOR,
    &ACORDE_SI_SEPTIMA,
    &ACORDE_MI_MENOR,
];

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct GuitarraCriolla {
    /// Distancia (m) desde la boca de la guitarra a la que la mano la toca.
    #[export]
    radio_toque: f32,
    /// Posición de la boca relativa al nodo.
    #[export]
    centro_local: Vector3,
    /// Velocidad mínima (m/s) de la mano para que el barrido suene.
    #[export]
    velocidad_minima: f32,
    #[export]
    volumen_db: f32,

    guitarra: Option<Guitarra>,
    generador: Option<GeneradorAudio>,
    manos: Vec<Gd<XrController3D>>,
    previo: Vec<(Vector3, bool)>,
    enfriamiento: f32,
    paso: usize,
    tecla_previa: bool,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for GuitarraCriolla {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            radio_toque: 0.3,
            centro_local: Vector3::new(0.0, 0.35, 0.0),
            velocidad_minima: 0.7,
            volumen_db: 2.0,
            guitarra: None,
            generador: None,
            manos: Vec::new(),
            previo: Vec::new(),
            enfriamiento: 0.0,
            paso: 0,
            tecla_previa: false,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let (stream, mix) = GeneradorAudio::nuevo_stream(0.15);
        self.guitarra = Some(Guitarra::new(mix, 1872));
        let mut rep = AudioStreamPlayer3D::new_alloc();
        rep.set_name("Cuerdas");
        rep.set_stream(&stream);
        rep.set_unit_size(3.0);
        rep.set_max_distance(40.0);
        rep.set_attenuation_model(AttenuationModel::INVERSE_DISTANCE);
        rep.set_volume_db(self.volumen_db);
        rep.set_position(self.centro_local);
        self.base_mut().add_child(&rep);
        rep.play();
        self.generador = GeneradorAudio::desde_reproductor(rep.get_stream_playback());

        // Las manos se buscan cuando todo el árbol está listo.
        self.run_deferred(|g| g.buscar_manos());
    }

    fn process(&mut self, delta: f64) {
        let dt = (delta as f32).max(1e-4);
        self.enfriamiento -= dt;
        let centro = self.base().get_global_transform() * self.centro_local;

        // 1) Detectar toques (sin tocar el sintetizador todavía).
        let mut toques: Vec<(usize, bool, f32)> = Vec::new();
        for (i, mano) in self.manos.iter().enumerate() {
            if !mano.is_inside_tree() {
                continue;
            }
            let p = mano.get_global_position();
            let gatillo = mano.get_float("trigger") > 0.55;
            let (p_prev, gat_prev) = self.previo[i];
            let v = p - p_prev;
            let rapidez = v.length() / dt;
            let cerca = p.distance_to(centro) < self.radio_toque;
            if cerca && rapidez > self.velocidad_minima && rapidez < 12.0 && self.enfriamiento <= 0.0 {
                let fuerza = ((rapidez - self.velocidad_minima) / 2.5).clamp(0.35, 1.0);
                toques.push((i, v.y < 0.0, fuerza));
            } else if cerca && gatillo && !gat_prev {
                toques.push((i, true, 0.8));
            }
            self.previo[i] = (p, gatillo);
        }

        // 2) Tecla G para probar en la PC.
        let tecla = Input::singleton().is_key_pressed(Key::G);
        if tecla && !self.tecla_previa {
            toques.push((usize::MAX, true, 0.85));
        }
        self.tecla_previa = tecla;

        // 3) Rasguear y vibrar.
        for (i, abajo, fuerza) in toques.into_iter().take(1) {
            self.rasguear(abajo, fuerza);
            if let Some(mano) = self.manos.get_mut(i) {
                mano.trigger_haptic_pulse("haptic", 0.0, (0.3 + 0.5 * fuerza) as f64, 0.07, 0.0);
            }
        }

        if let (Some(g), Some(gen)) = (self.guitarra.as_mut(), self.generador.as_mut()) {
            gen.rellenar(|| {
                let x = g.muestra();
                (x, x)
            });
        }
    }
}

#[godot_api]
impl GuitarraCriolla {
    /// Rasguea el siguiente acorde de la milonga (también llamable desde GDScript).
    #[func]
    pub fn rasguear(&mut self, hacia_abajo: bool, fuerza: f32) {
        if let Some(g) = self.guitarra.as_mut() {
            let acorde = SECUENCIA[self.paso % SECUENCIA.len()];
            g.rasguear(acorde, hacia_abajo, fuerza.clamp(0.1, 1.0));
            self.paso += 1;
            self.enfriamiento = 0.16;
        }
    }

    /// Rasguea un acorde concreto: 0 = Mi menor, 1 = Si séptima, 2 = La menor.
    /// Lo usa el director del tráiler para tocar una milonga a tiempo.
    #[func]
    pub fn rasguear_acorde(&mut self, acorde: i32, hacia_abajo: bool, fuerza: f32) {
        let a = match acorde {
            1 => &ACORDE_SI_SEPTIMA,
            2 => &ACORDE_LA_MENOR,
            _ => &ACORDE_MI_MENOR,
        };
        if let Some(g) = self.guitarra.as_mut() {
            g.rasguear(a, hacia_abajo, fuerza.clamp(0.1, 1.0));
        }
    }

    #[func]
    fn buscar_manos(&mut self) {
        let arbol = self.base().get_tree();
        self.manos = arbol
            .get_nodes_in_group("manos")
            .iter_shared()
            .filter_map(|n| n.try_cast::<XrController3D>().ok())
            .collect();
        self.previo = self
            .manos
            .iter()
            .map(|m| (m.get_global_position(), false))
            .collect();
    }
}
