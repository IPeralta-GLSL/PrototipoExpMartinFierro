//! Paisaje sonoro y fuego: viento + grillos (ambiente estéreo), y el fogón
//! con luz titilante y crepitar espacializado. Todo el audio se sintetiza en
//! Rust y se empuja a un AudioStreamGenerator en cada cuadro.

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, light_3d::Param, AudioServer, AudioStreamGenerator,
    AudioStreamGeneratorPlayback, AudioStreamPlayer, AudioStreamPlayer3D, Engine, OmniLight3D,
    ShaderMaterial,
};
use godot::prelude::*;

use crate::ruido::Perlin2;
use crate::sintesis::{Efectos, FuegoSonido, Grillos, TipoEfecto, Viento};

/// Envoltura de un AudioStreamGeneratorPlayback con un búfer reutilizable.
pub struct GeneradorAudio {
    playback: Gd<AudioStreamGeneratorPlayback>,
    bufer: PackedVector2Array,
}

impl GeneradorAudio {
    pub fn nuevo_stream(largo_s: f32) -> (Gd<AudioStreamGenerator>, f32) {
        let mix = AudioServer::singleton().get_mix_rate().max(8000.0);
        let mut g = AudioStreamGenerator::new_gd();
        g.set_mix_rate(mix);
        g.set_buffer_length(largo_s);
        (g, mix)
    }

    pub fn desde_reproductor(p: Option<Gd<godot::classes::AudioStreamPlayback>>) -> Option<Self> {
        let playback = p?.try_cast::<AudioStreamGeneratorPlayback>().ok()?;
        Some(Self {
            playback,
            bufer: PackedVector2Array::new(),
        })
    }

    /// Completa todo el espacio libre del búfer con muestras estéreo.
    pub fn rellenar(&mut self, mut muestra: impl FnMut() -> (f32, f32)) {
        let libres = self.playback.get_frames_available();
        if libres <= 0 {
            return;
        }
        self.bufer.resize(libres as usize);
        for v in self.bufer.as_mut_slice() {
            let (l, r) = muestra();
            *v = Vector2::new(l, r);
        }
        self.playback.push_buffer(&self.bufer);
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node)]
pub struct AmbientePampa {
    #[export]
    volumen_viento: f32,
    #[export]
    volumen_grillos: f32,
    /// Materiales del pasto: se les pasa la intensidad de la ráfaga ("rafaga").
    #[export]
    material_pasto: Option<Gd<ShaderMaterial>>,
    #[export]
    material_cortadera: Option<Gd<ShaderMaterial>>,

    viento: Option<Viento>,
    grillos: Option<Grillos>,
    generador: Option<GeneradorAudio>,
    base: Base<Node>,
}

#[godot_api]
impl INode for AmbientePampa {
    fn init(base: Base<Node>) -> Self {
        Self {
            volumen_viento: 0.45,
            volumen_grillos: 0.03,
            material_pasto: None,
            material_cortadera: None,
            viento: None,
            grillos: None,
            generador: None,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let (stream, mix) = GeneradorAudio::nuevo_stream(0.25);
        let mut v = Viento::new(mix, 1872);
        v.volumen = self.volumen_viento;
        let mut g = Grillos::new(mix, 1879, 5);
        g.volumen = self.volumen_grillos;
        self.viento = Some(v);
        self.grillos = Some(g);

        let mut rep = AudioStreamPlayer::new_alloc();
        rep.set_name("SonidoAmbiente");
        rep.set_stream(&stream);
        self.base_mut().add_child(&rep);
        rep.play();
        self.generador = GeneradorAudio::desde_reproductor(rep.get_stream_playback());
        if self.generador.is_none() {
            godot_warn!("AmbientePampa: no se pudo obtener el playback del generador");
        }
    }

    fn process(&mut self, _delta: f64) {
        let (Some(v), Some(g)) = (self.viento.as_mut(), self.grillos.as_mut()) else {
            return;
        };
        if let Some(gen) = self.generador.as_mut() {
            gen.rellenar(|| {
                let (a, b) = v.muestra();
                let (c, d) = g.muestra();
                (a + c, b + d)
            });
        }
        let rafaga = v.intensidad().to_variant();
        for m in [&mut self.material_pasto, &mut self.material_cortadera]
            .into_iter()
            .flatten()
        {
            m.set_shader_parameter("rafaga", &rafaga);
        }
    }
}

#[godot_api]
impl AmbientePampa {
    /// Cambia los volúmenes en vivo (lo usa el ciclo tarde→noche).
    #[func]
    pub fn fijar_volumenes(&mut self, viento: f32, grillos: f32) {
        self.volumen_viento = viento;
        self.volumen_grillos = grillos;
        if let Some(v) = self.viento.as_mut() {
            v.volumen = viento;
        }
        if let Some(g) = self.grillos.as_mut() {
            g.volumen = grillos;
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Fogon {
    #[export]
    luz: Option<Gd<OmniLight3D>>,
    #[export]
    material_llamas: Option<Gd<ShaderMaterial>>,
    #[export]
    energia_base: f32,
    /// Fracción de variación de la luz (0 = fija).
    #[export]
    parpadeo: f32,
    #[export]
    intensidad_llamas: f32,
    #[export]
    volumen_db: f32,

    t: f64,
    ruido: Perlin2,
    pos_luz: Vector3,
    sonido: Option<FuegoSonido>,
    generador: Option<GeneradorAudio>,
    /// 1 = recién avivado (leña nueva); baja solo.
    avivado: f32,
    efectos: Option<Efectos>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Fogon {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            luz: None,
            material_llamas: None,
            energia_base: 2.4,
            parpadeo: 0.35,
            intensidad_llamas: 2.2,
            volumen_db: 0.0,
            t: 0.0,
            ruido: Perlin2::new(1834),
            pos_luz: Vector3::ZERO,
            sonido: None,
            generador: None,
            avivado: 0.0,
            efectos: None,
            base,
        }
    }

    fn ready(&mut self) {
        if let Some(l) = &self.luz {
            self.pos_luz = l.get_position();
        }
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.base_mut().add_to_group("fogon");
        let (stream, mix) = GeneradorAudio::nuevo_stream(0.2);
        self.sonido = Some(FuegoSonido::new(mix, 1834));
        self.efectos = Some(Efectos::new(mix, 77));
        let mut rep = AudioStreamPlayer3D::new_alloc();
        rep.set_name("Crepitar");
        rep.set_stream(&stream);
        rep.set_unit_size(2.5);
        rep.set_max_distance(30.0);
        rep.set_attenuation_model(AttenuationModel::INVERSE_DISTANCE);
        rep.set_volume_db(self.volumen_db);
        rep.set_position(Vector3::new(0.0, 0.35, 0.0));
        self.base_mut().add_child(&rep);
        rep.play();
        self.generador = GeneradorAudio::desde_reproductor(rep.get_stream_playback());
    }

    fn process(&mut self, delta: f64) {
        self.t += delta;
        self.avivado = (self.avivado - delta as f32 * 0.3).max(0.0);
        let mas = 1.0 + 1.6 * self.avivado;
        let t = self.t as f32;
        let n = &self.ruido;
        // Parpadeo: mezcla de un vaivén lento y titileo rápido.
        let f = n.fbm(t * 2.3, 0.37, 3) * 0.7 + n.ruido(t * 9.1, 4.2) * 0.3;
        let vaiven = Vector3::new(
            n.ruido(t * 3.7, 10.5),
            n.ruido(t * 3.1, 20.5) * 0.5,
            n.ruido(t * 3.3, 30.5),
        ) * 0.05;
        if let Some(luz) = self.luz.as_mut() {
            luz.set_param(Param::ENERGY, (self.energia_base * mas * (1.0 + self.parpadeo * f)).max(0.0));
            luz.set_position(self.pos_luz + vaiven);
        }
        if let Some(m) = self.material_llamas.as_mut() {
            let i = self.intensidad_llamas * (1.0 + 0.8 * self.avivado) * (1.0 + 0.3 * f);
            m.set_shader_parameter("intensidad", &i.to_variant());
        }
        if let (Some(s), Some(e), Some(gen)) = (self.sonido.as_mut(), self.efectos.as_mut(), self.generador.as_mut()) {
            gen.rellenar(|| {
                let x = s.muestra() + e.muestra();
                (x, x)
            });
        }
    }
}

#[godot_api]
impl Fogon {
    /// Leña nueva: el fuego crece unos segundos y chisporrotea.
    #[func]
    pub fn avivar(&mut self) {
        self.avivado = 1.0;
        if let Some(e) = self.efectos.as_mut() {
            e.disparar(TipoEfecto::Chispas, 1.0);
        }
    }
}
