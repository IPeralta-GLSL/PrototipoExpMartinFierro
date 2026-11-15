//! Interacción en VR: las manos del jugador, objetos que se agarran y se
//! tiran, animales que reaccionan a las caricias y sonidos de efecto.
//!
//! - `ManosXR`: muestra las manos reales del Quest (malla de hand tracking de
//!   Meta, animada dedo por dedo; con los controles en la mano también) y
//!   resuelve agarrar/soltar/tirar, el sorbo de mate, ponerse el sombrero y
//!   acariciar animales.
//! - `Agarrable`: cuerpo rígido que se puede agarrar (mate, sombrero, leña...).
//!   La leña que cae en un fogón lo aviva. Si se pierde, vuelve a su lugar.
//! - `Animal`: alterna animaciones (rutina) y reacciona a las caricias.
//! - `SonidoEfectos`: fuente 3D de efectos sintetizados.

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, rigid_body_3d::FreezeMode, AudioStreamPlayer3D, BoxShape3D,
    ClassDb, CollisionShape3D, Engine, GeometryInstance3D, Material, Node, Node3D, RigidBody3D, Shader,
    ShaderMaterial, StandardMaterial3D,
    StaticBody3D, VisualInstance3D, WorldBoundaryShape3D, XrCamera3D, XrController3D,
    XrHandModifier3D, XrHandTracker, XrNode3D, XrServer, IRigidBody3D,
};
use godot::classes::xr_hand_tracker::HandJoint;
use godot::prelude::*;

use crate::ambiente::GeneradorAudio;
use crate::conversion::argumento_usuario;
use crate::modelos::ModeloIA;
use crate::personajes::Caminante;
use crate::sintesis::{Efectos, TipoEfecto};

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct SonidoEfectos {
    #[export]
    volumen_db: f32,
    efectos: Option<Efectos>,
    generador: Option<GeneradorAudio>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for SonidoEfectos {
    fn init(base: Base<Node3D>) -> Self {
        Self { volumen_db: 0.0, efectos: None, generador: None, base }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let (stream, mix) = GeneradorAudio::nuevo_stream(0.15);
        let semilla = self.base().instance_id().to_i64().unsigned_abs();
        self.efectos = Some(Efectos::new(mix, semilla));
        let mut rep = AudioStreamPlayer3D::new_alloc();
        rep.set_name("Efecto");
        rep.set_stream(&stream);
        rep.set_unit_size(2.0);
        rep.set_max_distance(25.0);
        rep.set_attenuation_model(AttenuationModel::INVERSE_DISTANCE);
        rep.set_volume_db(self.volumen_db);
        self.base_mut().add_child(&rep);
        rep.play();
        self.generador = GeneradorAudio::desde_reproductor(rep.get_stream_playback());
    }

    fn process(&mut self, _delta: f64) {
        if let (Some(e), Some(g)) = (self.efectos.as_mut(), self.generador.as_mut()) {
            g.rellenar(|| {
                let x = e.muestra();
                (x, x)
            });
        }
    }
}

#[godot_api]
impl SonidoEfectos {
    /// "resoplido", "gemido", "sorbo" o "chispas".
    #[func]
    pub fn sonar(&mut self, nombre: GString, fuerza: f32) {
        if let (Some(e), Some(t)) = (self.efectos.as_mut(), TipoEfecto::desde_nombre(&nombre.to_string())) {
            e.disparar(t, fuerza);
        }
    }
}

fn crear_sonido(padre: &mut Gd<Node3D>, pos: Vector3) -> Gd<SonidoEfectos> {
    let mut s = SonidoEfectos::new_alloc();
    s.set_name("Sonido");
    s.set_position(pos);
    padre.add_child(&s);
    s
}

// ---------------------------------------------------------------------------

/// Contorno amarillo de lo interactivo (shaders/contorno.gdshader como
/// material_overlay). Nivel: 0 = nada, ~0.4 = el jugador está cerca,
/// 1 = al alcance de la mano. Sin contorno no agrega ninguna pasada de dibujo.
pub struct Contorno {
    material: Option<Gd<ShaderMaterial>>,
    mallas: Vec<Gd<GeometryInstance3D>>,
    activo: bool,
    nivel: f32,
    objetivo: f32,
    t: f32,
    siempre: bool,
}

impl Contorno {
    pub fn nuevo() -> Self {
        Self {
            material: None,
            mallas: Vec::new(),
            activo: false,
            nivel: 0.0,
            objetivo: 0.0,
            t: 0.0,
            // Para capturas en la PC: -- --resaltar
            siempre: argumento_usuario("resaltar").is_some(),
        }
    }

    pub fn pedir(&mut self, nivel: f32) {
        self.objetivo = nivel;
    }

    fn juntar(n: &Gd<Node>, out: &mut Vec<Gd<GeometryInstance3D>>) {
        if let Ok(g) = n.clone().try_cast::<GeometryInstance3D>() {
            out.push(g);
        }
        for h in n.get_children().iter_shared() {
            Self::juntar(&h, out);
        }
    }

    pub fn actualizar(&mut self, raiz: &Gd<Node>, dt: f32, apagado: bool) {
        self.t += dt;
        let objetivo = if apagado {
            0.0
        } else if self.siempre {
            self.objetivo.max(0.7)
        } else {
            self.objetivo
        };
        self.nivel += (objetivo - self.nivel) * (dt * 10.0).min(1.0);
        if self.nivel < 0.03 {
            if self.activo {
                for m in self.mallas.iter_mut() {
                    m.set_material_overlay(Option::<&Gd<Material>>::None);
                }
                self.activo = false;
            }
            return;
        }
        if self.material.is_none() {
            let Ok(shader) = try_load::<Shader>("res://shaders/contorno.gdshader") else { return };
            let mut mat = ShaderMaterial::new_gd();
            mat.set_shader(&shader);
            self.material = Some(mat);
        }
        if !self.activo {
            self.mallas.clear();
            Self::juntar(raiz, &mut self.mallas);
            if let Some(mat) = self.material.clone() {
                let mat = mat.upcast::<Material>();
                for m in self.mallas.iter_mut() {
                    m.set_material_overlay(Some(&mat));
                }
            }
            self.activo = true;
        }
        if let Some(mat) = self.material.as_mut() {
            // Suave: transparente y fino de lejos, un poco más marcado al alcance de la mano.
            let pulso = 0.9 + 0.1 * (self.t * 2.5).sin();
            mat.set_shader_parameter("alfa", &(self.nivel.min(1.0) * 0.55 * pulso).to_variant());
            mat.set_shader_parameter("grosor", &(1.0 + 1.5 * self.nivel).to_variant());
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Animal {
    /// El modelo animado (hijo).
    #[export]
    modelo: Option<Gd<ModeloIA>>,
    /// Animaciones que alterna solo: "idle:6-12,pastar:8-16" (segundos).
    #[export]
    rutina: GString,
    /// Animación al ser acariciado.
    #[export]
    reaccion: GString,
    #[export]
    duracion_reaccion: f32,
    /// "resoplido" (caballo) o "gemido" (perro).
    #[export]
    sonido: GString,
    /// Dónde se lo acaricia (cabeza/cuello), en coordenadas locales.
    #[export]
    punto_caricia: Vector3,
    #[export]
    radio_caricia: f32,

    pasos: Vec<(String, f32, f32)>,
    paso: usize,
    resta: f32,
    reaccionando: f32,
    enfriamiento: f32,
    voz: Option<Gd<SonidoEfectos>>,
    contorno: Contorno,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Animal {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            modelo: None,
            rutina: GString::new(),
            reaccion: GString::from("contento"),
            duracion_reaccion: 3.0,
            sonido: GString::new(),
            punto_caricia: Vector3::new(0.5, 1.0, 0.0),
            radio_caricia: 0.5,
            pasos: Vec::new(),
            paso: 0,
            resta: 0.0,
            reaccionando: 0.0,
            enfriamiento: 0.0,
            voz: None,
            contorno: Contorno::nuevo(),
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.base_mut().add_to_group("acariciable");
        let p = self.punto_caricia;
        let mut yo = self.base().clone().upcast::<Node3D>();
        self.voz = Some(crear_sonido(&mut yo, p));
        // "idle:6-12,pastar:8-16"
        self.pasos = self
            .rutina
            .to_string()
            .split(',')
            .filter_map(|p| {
                let (n, r) = p.trim().split_once(':')?;
                let (a, b) = r.split_once('-').unwrap_or((r, r));
                Some((n.trim().to_string(), a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .collect();
        // Cada animal arranca en otro punto de su rutina.
        let semilla = self.base().instance_id().to_i64().unsigned_abs();
        if !self.pasos.is_empty() {
            self.paso = (semilla % self.pasos.len() as u64) as usize;
            self.empezar_paso(((semilla % 100) as f32) / 100.0);
        }
    }

    fn process(&mut self, delta: f64) {
        let dt = delta as f32;
        let raiz = self.base().clone().upcast::<Node>();
        self.contorno.actualizar(&raiz, dt, false);
        self.enfriamiento -= dt;
        if self.reaccionando > 0.0 {
            self.reaccionando -= dt;
            if self.reaccionando <= 0.0 && !self.pasos.is_empty() {
                self.empezar_paso(0.5);
            }
            return;
        }
        if self.pasos.is_empty() {
            return;
        }
        self.resta -= dt;
        if self.resta <= 0.0 {
            self.paso = (self.paso + 1) % self.pasos.len();
            self.empezar_paso(0.5);
        }
    }
}

#[godot_api]
impl Animal {
    /// Lo acarician: reacciona (animación y sonido). Devuelve true si reaccionó.
    #[func]
    pub fn acariciar(&mut self) -> bool {
        if self.enfriamiento > 0.0 {
            return false;
        }
        self.enfriamiento = self.duracion_reaccion + 2.0;
        self.reaccionando = self.duracion_reaccion;
        // Si camina (Caminante), se queda quieto mientras disfruta.
        if let Some(p) = self.base().get_parent() {
            if let Ok(mut c) = p.try_cast::<Caminante>() {
                c.bind_mut().pausar(self.duracion_reaccion + 0.5);
            }
        }
        let r = self.reaccion.clone();
        self.animar(&r.to_string());
        let s = self.sonido.clone();
        if let Some(v) = self.voz.as_mut() {
            v.bind_mut().sonar(s, 1.0);
        }
        true
    }

    #[func]
    pub fn punto_global(&self) -> Vector3 {
        self.base().get_global_transform() * self.punto_caricia
    }

    #[func]
    pub fn radio(&self) -> f32 {
        self.radio_caricia
    }

    /// 0 = sin contorno, ~0.4 = el jugador está cerca, 1 = la mano lo alcanza.
    #[func]
    pub fn resaltar(&mut self, nivel: f32) {
        self.contorno.pedir(nivel);
    }
}

impl Animal {
    fn empezar_paso(&mut self, fraccion: f32) {
        let (nombre, a, b) = self.pasos[self.paso].clone();
        self.resta = a + (b - a) * fraccion;
        self.animar(&nombre);
    }

    fn animar(&mut self, candidatos: &str) {
        if let Some(m) = self.modelo.as_mut() {
            m.bind_mut().reproducir(GString::from(&format!("{candidatos}|idle")));
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = RigidBody3D)]
pub struct Agarrable {
    /// "mate", "sombrero", "lena" u otro (sin comportamiento especial).
    #[export]
    tipo: GString,
    /// Distancia (m) desde la palma a la que se lo puede agarrar.
    #[export]
    radio_agarre: f32,

    origen: Transform3D,
    padre_origen: Option<Gd<Node>>,
    tomado: bool,
    consumido: f32,
    contorno: Contorno,
    base: Base<RigidBody3D>,
}

#[godot_api]
impl IRigidBody3D for Agarrable {
    fn init(base: Base<RigidBody3D>) -> Self {
        Self {
            tipo: GString::new(),
            radio_agarre: 0.2,
            origen: Transform3D::IDENTITY,
            padre_origen: None,
            tomado: false,
            consumido: 0.0,
            contorno: Contorno::nuevo(),
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.base_mut().add_to_group("agarrable");
        self.origen = self.base().get_global_transform();
        self.padre_origen = self.base().get_parent();
        self.base_mut().set_freeze_mode(FreezeMode::KINEMATIC);
        self.base_mut().set_freeze_enabled(true);
        self.base_mut().set_use_continuous_collision_detection(true);
        self.crear_forma();
        asegurar_suelo(&self.base().clone().upcast::<Node>());
    }

    fn physics_process(&mut self, delta: f64) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let dt = delta as f32;
        let raiz = self.base().clone().upcast::<Node>();
        let apagado = self.tomado || self.consumido > 0.0;
        self.contorno.actualizar(&raiz, dt, apagado);
        if self.consumido > 0.0 {
            self.consumido -= dt;
            if self.consumido <= 0.0 {
                self.volver();
            }
            return;
        }
        if self.tomado {
            return;
        }
        let p = self.base().get_global_position();
        if p.y < -3.0 || p.distance_to(self.origen.origin) > 40.0 {
            self.volver();
            return;
        }
        // Leña: si cae en un fogón, lo aviva y se consume.
        if self.tipo == GString::from("lena") {
            let arbol = self.base().get_tree();
            for f in arbol.get_nodes_in_group("fogon").iter_shared() {
                let Ok(mut fog) = f.try_cast::<Node3D>() else { continue };
                let c = fog.get_global_position();
                if Vector2::new(p.x - c.x, p.z - c.z).length() < 0.55 && p.y - c.y < 0.9 {
                    fog.call("avivar", &[]);
                    self.consumir();
                    break;
                }
            }
        }
    }
}

#[godot_api]
impl Agarrable {
    #[func]
    pub fn tomar(&mut self) {
        self.tomado = true;
        self.base_mut().set_freeze_enabled(true);
        // Si estaba puesto (el sombrero en la cabeza), vuelve a la escena.
        let actual = self.base().get_parent();
        if let (Some(p), Some(orig)) = (actual, self.padre_origen.clone()) {
            if p != orig {
                let xf = self.base().get_global_transform();
                let mut yo = self.base().clone().upcast::<Node>();
                yo.reparent(&orig);
                self.base_mut().set_global_transform(xf);
            }
        }
    }

    #[func]
    pub fn soltar(&mut self, velocidad: Vector3, angular: Vector3) {
        self.tomado = false;
        self.base_mut().set_freeze_enabled(false);
        self.base_mut().set_linear_velocity(velocidad);
        self.base_mut().set_angular_velocity(angular);
        self.base_mut().set_sleeping(false);
    }

    /// Queda fijo en otro nodo (el sombrero sobre la cabeza).
    #[func]
    pub fn calzar(&mut self, en: Gd<Node3D>, local: Transform3D) {
        self.tomado = false;
        self.base_mut().set_freeze_enabled(true);
        let mut yo = self.base().clone().upcast::<Node>();
        yo.reparent(&en);
        self.base_mut().set_transform(local);
    }

    #[func]
    pub fn volver(&mut self) {
        self.tomado = false;
        self.consumido = 0.0;
        if let (Some(p), Some(orig)) = (self.base().get_parent(), self.padre_origen.clone()) {
            if p != orig {
                let mut yo = self.base().clone().upcast::<Node>();
                yo.reparent(&orig);
            }
        }
        let o = self.origen;
        self.base_mut().set_freeze_enabled(true);
        self.base_mut().set_global_transform(o);
        self.base_mut().set_linear_velocity(Vector3::ZERO);
        self.base_mut().set_angular_velocity(Vector3::ZERO);
        self.base_mut().show();
    }

    #[func]
    pub fn es(&self, tipo: GString) -> bool {
        self.tipo == tipo
    }

    #[func]
    pub fn radio(&self) -> f32 {
        self.radio_agarre
    }

    #[func]
    pub fn resaltar(&mut self, nivel: f32) {
        self.contorno.pedir(nivel);
    }
}

impl Agarrable {
    fn consumir(&mut self) {
        self.base_mut().hide();
        self.base_mut().set_freeze_enabled(true);
        self.consumido = 6.0;
    }

    /// Colisión de caja a partir de lo que se ve (si la escena no trae una).
    fn crear_forma(&mut self) {
        let yo = self.base().clone().upcast::<Node>();
        if yo.get_children().iter_shared().any(|h| h.try_cast::<CollisionShape3D>().is_ok()) {
            return;
        }
        let inv = self.base().get_global_transform().affine_inverse();
        let mut caja: Option<Aabb> = None;
        fn juntar(n: &Gd<Node>, inv: Transform3D, caja: &mut Option<Aabb>) {
            if let Ok(v) = n.clone().try_cast::<VisualInstance3D>() {
                let local = inv * v.get_global_transform() * v.get_aabb();
                *caja = Some(match caja {
                    Some(c) => c.merge(local),
                    None => local,
                });
            }
            for h in n.get_children().iter_shared() {
                juntar(&h, inv, caja);
            }
        }
        juntar(&yo, inv, &mut caja);
        let c = caja.unwrap_or(Aabb::new(Vector3::new(-0.1, 0.0, -0.1), Vector3::new(0.2, 0.2, 0.2)));
        let mut forma = BoxShape3D::new_gd();
        forma.set_size(c.size.coord_max(Vector3::splat(0.03)));
        let mut col = CollisionShape3D::new_alloc();
        col.set_shape(&forma);
        col.set_position(c.center());
        self.base_mut().add_child(&col);
        self.base_mut().set_mass((c.size.x * c.size.y * c.size.z * 400.0).clamp(0.1, 5.0));
    }
}

/// Un piso físico (plano infinito en y=0) para que lo que se suelta no caiga
/// para siempre. Se crea una sola vez por escena.
fn asegurar_suelo(desde: &Gd<Node>) {
    let mut arbol = desde.get_tree();
    if arbol.get_first_node_in_group("suelo_fisico").is_some() {
        return;
    }
    let Some(mut escena) = arbol.get_current_scene() else { return };
    let mut suelo = StaticBody3D::new_alloc();
    suelo.set_name("SueloFisico");
    suelo.add_to_group("suelo_fisico");
    let mut col = CollisionShape3D::new_alloc();
    col.set_shape(&WorldBoundaryShape3D::new_gd());
    suelo.add_child(&col);
    escena.call_deferred("add_child", &[suelo.to_variant()]);
    let _ = &mut arbol;
}

// ---------------------------------------------------------------------------

struct Mano {
    tracker: StringName,
    control: Option<Gd<XrController3D>>,
    malla: Option<Gd<Node3D>>,
    prev_pos: Vector3,
    vel: Vector3,
    vel_ang: Vector3,
    prev_basis: Basis,
    cerrada: bool,
    tomado: Option<(Gd<Agarrable>, Transform3D)>,
    caricia: f32,
    sorbo_cd: f32,
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct ManosXR {
    manos: Vec<Mano>,
    cabeza: Option<Gd<XrCamera3D>>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for ManosXR {
    fn init(base: Base<Node3D>) -> Self {
        Self { manos: Vec::new(), cabeza: None, base }
    }

    fn ready(&mut self) {
        let Some(origen) = self.base().get_parent().and_then(|p| p.try_cast::<Node3D>().ok()) else {
            return;
        };
        self.cabeza = origen.try_get_node_as::<XrCamera3D>("Cabeza");
        let mut piel = StandardMaterial3D::new_gd();
        piel.set_albedo(Color::from_rgb(0.66, 0.47, 0.34));
        piel.set_roughness(0.75);
        let hay_malla = ClassDb::singleton().class_exists("OpenXRFbHandTrackingMesh");
        for (i, lado, control) in [(0, "left", "ManoIzquierda"), (1, "right", "ManoDerecha")] {
            let tracker = StringName::from(&format!("/user/hand_tracker/{lado}"));
            let mut malla = None;
            if hay_malla {
                // XRNode3D -> malla de la mano de Meta (Skeleton3D) -> XRHandModifier3D
                let mut nodo = XrNode3D::new_alloc();
                nodo.set_name(&format!("Mano_{lado}"));
                nodo.set_tracker(&tracker);
                nodo.set_show_when_tracked(true);
                let m = ClassDb::singleton().instantiate("OpenXRFbHandTrackingMesh");
                if let Ok(mut m) = m.try_to::<Gd<Node3D>>() {
                    m.set("hand", &(i as i64).to_variant());
                    m.set("material", &piel.clone().upcast::<Material>().to_variant());
                    let mut modif = XrHandModifier3D::new_alloc();
                    modif.set_hand_tracker(&tracker);
                    m.add_child(&modif);
                    nodo.add_child(&m);
                    malla = Some(m);
                }
                let mut o = origen.clone();
                o.add_child(&nodo);
            }
            self.manos.push(Mano {
                tracker,
                control: origen.try_get_node_as::<XrController3D>(control),
                malla,
                prev_pos: Vector3::ZERO,
                vel: Vector3::ZERO,
                vel_ang: Vector3::ZERO,
                prev_basis: Basis::IDENTITY,
                cerrada: false,
                tomado: None,
                caricia: 0.0,
                sorbo_cd: 0.0,
            });
        }
    }

    fn physics_process(&mut self, delta: f64) {
        let dt = (delta as f32).max(1e-4);
        let cabeza = self.cabeza.as_ref().map(|c| c.get_global_position());
        let Some(origen) = self.base().get_parent().and_then(|p| p.try_cast::<Node3D>().ok()) else {
            return;
        };
        let oxf = origen.get_global_transform();
        let arbol = self.base().get_tree();
        self.resaltar_interactivos(cabeza, oxf);
        for i in 0..self.manos.len() {
            let Some((palma, cerrada, guante)) = self.leer_mano(i, oxf) else { continue };
            let m = &mut self.manos[i];
            // Velocidad de la palma (para tirar lo que se suelta).
            let v = (palma.origin - m.prev_pos) / dt;
            m.vel = m.vel.lerp(v, 0.5);
            let dq = (palma.basis * m.prev_basis.inverse()).get_quaternion();
            let (eje, ang) = (dq.get_axis(), dq.get_angle());
            let w = if eje.length() > 0.5 { eje.normalized() * (ang / dt) } else { Vector3::ZERO };
            m.vel_ang = m.vel_ang.lerp(w, 0.5);
            m.prev_pos = palma.origin;
            m.prev_basis = palma.basis;
            m.sorbo_cd -= dt;
            // Guantes de los controles: solo si no hay malla de mano.
            if let Some(c) = m.control.as_mut() {
                if let Some(mut g) = c.try_get_node_as::<Node3D>("Guante") {
                    g.set_visible(guante);
                }
            }

            let antes = m.cerrada;
            m.cerrada = cerrada;
            if cerrada && !antes && m.tomado.is_none() {
                // Agarrar lo más cercano a la palma.
                let mut mejor: Option<(Gd<Agarrable>, f32)> = None;
                for n in arbol.get_nodes_in_group("agarrable").iter_shared() {
                    let Ok(a) = n.try_cast::<Agarrable>() else { continue };
                    if !a.is_visible_in_tree() {
                        continue;
                    }
                    let d = a.get_global_position().distance_to(palma.origin);
                    let r = a.bind().radio();
                    if d < r + 0.06 && mejor.as_ref().map_or(true, |(_, md)| d < *md) {
                        mejor = Some((a, d));
                    }
                }
                if let Some((mut a, _)) = mejor {
                    a.bind_mut().tomar();
                    let rel = palma.affine_inverse() * a.get_global_transform();
                    m.tomado = Some((a, rel));
                    vibrar(m.control.as_mut(), 0.5, 0.06);
                }
            } else if !cerrada && antes {
                if let Some((mut a, _)) = m.tomado.take() {
                    // ¿Sombrero soltado sobre la cabeza? Se lo pone.
                    let mut puesto = false;
                    if a.bind().es(GString::from("sombrero")) {
                        if let (Some(c), Some(cam)) = (cabeza, self.cabeza.clone()) {
                            let p = a.get_global_position();
                            if p.distance_to(c) < 0.32 && p.y > c.y - 0.08 {
                                let local = Transform3D::new(Basis::IDENTITY, Vector3::new(0.0, 0.11, 0.03));
                                a.bind_mut().calzar(cam.upcast::<Node3D>(), local);
                                puesto = true;
                            }
                        }
                    }
                    if !puesto {
                        let (v, w) = (m.vel * 1.15, m.vel_ang);
                        a.bind_mut().soltar(v, w);
                    }
                }
            }
            if let Some((a, rel)) = m.tomado.as_mut() {
                let xf = palma * *rel;
                a.set_global_transform(xf);
                // Mate en la boca: sorbo.
                if let Some(c) = cabeza {
                    if a.bind().es(GString::from("mate")) && m.sorbo_cd <= 0.0 && xf.origin.distance_to(c - Vector3::new(0.0, 0.07, 0.0)) < 0.17 {
                        m.sorbo_cd = 2.4;
                        let mut s = a.clone().upcast::<Node3D>();
                        if s.try_get_node_as::<SonidoEfectos>("Sonido").is_none() {
                            crear_sonido(&mut s, Vector3::ZERO);
                        }
                        if let Some(mut snd) = s.try_get_node_as::<SonidoEfectos>("Sonido") {
                            snd.bind_mut().sonar(GString::from("sorbo"), 1.0);
                        }
                        vibrar(m.control.as_mut(), 0.25, 0.4);
                    }
                }
            }
            // Caricias: la palma moviéndose cerca de la cabeza de un animal.
            let rapidez = m.vel.length();
            let mut cerca = false;
            for n in arbol.get_nodes_in_group("acariciable").iter_shared() {
                let Ok(mut an) = n.try_cast::<Animal>() else { continue };
                let (p, r) = (an.bind().punto_global(), an.bind().radio());
                if p.distance_to(palma.origin) < r {
                    cerca = true;
                    if rapidez > 0.2 {
                        m.caricia += dt;
                        if m.caricia > 0.35 {
                            m.caricia = 0.0;
                            if an.bind_mut().acariciar() {
                                vibrar(m.control.as_mut(), 0.35, 0.25);
                            }
                        }
                    }
                }
            }
            if !cerca {
                m.caricia = 0.0;
            }
        }
    }
}

impl ManosXR {
    /// Contorno amarillo: suave si estás cerca, fuerte si la mano lo alcanza.
    fn resaltar_interactivos(&mut self, cabeza: Option<Vector3>, oxf: Transform3D) {
        let Some(c) = cabeza else { return };
        let mut palmas: Vec<Vector3> = Vec::new();
        for i in 0..self.manos.len() {
            if self.manos[i].tomado.is_some() {
                continue;
            }
            if let Some((p, _, _)) = self.leer_mano(i, oxf) {
                palmas.push(p.origin);
            }
        }
        let arbol = self.base().get_tree();
        for n in arbol.get_nodes_in_group("agarrable").iter_shared() {
            let Ok(mut a) = n.try_cast::<Agarrable>() else { continue };
            let p = a.get_global_position();
            let r = a.bind().radio() + 0.06;
            let nivel = if palmas.iter().any(|m| m.distance_to(p) < r) {
                1.0
            } else if p.distance_to(c) < 2.2 {
                0.4
            } else {
                0.0
            };
            a.bind_mut().resaltar(nivel);
        }
        for n in arbol.get_nodes_in_group("acariciable").iter_shared() {
            let Ok(mut an) = n.try_cast::<Animal>() else { continue };
            let (p, r) = (an.bind().punto_global(), an.bind().radio());
            let nivel = if palmas.iter().any(|m| m.distance_to(p) < r) {
                1.0
            } else if p.distance_to(c) < 2.5 {
                0.3
            } else {
                0.0
            };
            an.bind_mut().resaltar(nivel);
        }
    }

    /// Palma (espacio global), mano cerrada y si hay que mostrar el guante.
    fn leer_mano(&mut self, i: usize, oxf: Transform3D) -> Option<(Transform3D, bool, bool)> {
        let xr = XrServer::singleton();
        let m = &self.manos[i];
        let tracker = xr.get_tracker(&m.tracker).and_then(|t| t.try_cast::<XrHandTracker>().ok());
        let hay_mano = tracker.as_ref().is_some_and(|t| t.get_has_tracking_data());
        let control_activo = m.control.as_ref().is_some_and(|c| c.get_has_tracking_data());
        let gatillo_grip = m.control.as_ref().map_or(0.0, |c| c.get_float("grip"));
        let malla_visible = m.malla.as_ref().is_some_and(|n| n.is_visible_in_tree());
        if let Some(t) = tracker.filter(|_| hay_mano) {
            let palma = oxf * t.get_hand_joint_transform(HandJoint::PALM);
            // Mano cerrada: la punta de los dedos cerca de la palma (o el grip del control).
            let mut d = 0.0;
            for j in [HandJoint::INDEX_FINGER_TIP, HandJoint::MIDDLE_FINGER_TIP, HandJoint::RING_FINGER_TIP] {
                d += (oxf * t.get_hand_joint_transform(j)).origin.distance_to(palma.origin);
            }
            let cerrada = d / 3.0 < 0.065 || gatillo_grip > 0.55;
            return Some((palma, cerrada, !malla_visible));
        }
        if control_activo {
            let c = m.control.as_ref()?;
            return Some((c.get_global_transform(), gatillo_grip > 0.55, true));
        }
        None
    }
}

fn vibrar(control: Option<&mut Gd<XrController3D>>, amplitud: f64, duracion: f64) {
    if let Some(c) = control {
        c.trigger_haptic_pulse("haptic", 0.0, amplitud, duracion, 0.0);
    }
}
