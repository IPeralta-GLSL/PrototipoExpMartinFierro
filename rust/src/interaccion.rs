//! Interacción en VR: las manos del jugador, objetos que se agarran y se
//! tiran, animales que reaccionan a las caricias y sonidos de efecto.
//!
//! - `ManosXR`: muestra las manos reales del Quest (malla de hand tracking de
//!   Meta, animada dedo por dedo; con los controles en la mano también) y
//!   resuelve agarrar/soltar/tirar, el sorbo de mate, ponerse el sombrero y
//!   acariciar animales. Experimental (solo con -- --brazos): brazos con manga de
//!   camisa de paisano (hombro estimado desde la cabeza, codo por IK); no
//!   coinciden del todo con la malla de la mano de Meta, por eso no van por defecto.
//! - `Agarrable`: cuerpo rígido que se puede agarrar (mate, sombrero, leña...).
//!   La leña que cae en un fogón lo aviva. Si se pierde, vuelve a su lugar.
//! - `Animal`: alterna animaciones (rutina) y reacciona a las caricias.
//! - `SonidoEfectos`: fuente 3D de efectos sintetizados.

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, base_material_3d::{Feature, TextureParam},
    rigid_body_3d::FreezeMode, AudioStreamPlayer3D, BoxShape3D,
    geometry_instance_3d::ShadowCastingSetting, ClassDb, CollisionShape3D, CylinderMesh, Engine,
    GeometryInstance3D, Material, Mesh, MeshInstance3D, Node, Node3D, RigidBody3D, Shader,
    FastNoiseLite, NoiseTexture2D, ShaderMaterial, SphereMesh, StandardMaterial3D,
    Skeleton3D, StaticBody3D, VisualInstance3D, WorldBoundaryShape3D, XrCamera3D, XrController3D,
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
            // Cada objeto respira con su propio desfase (no todos al unísono).
            t: {
                use std::sync::atomic::{AtomicU32, Ordering};
                static CUENTA: AtomicU32 = AtomicU32::new(0);
                CUENTA.fetch_add(1, Ordering::Relaxed) as f32 * 0.83
            },
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
        // Siempre visible (lo interactivo se distingue de lejos); más marcado al alcance.
        let base = if self.siempre { 0.7 } else { 0.55 };
        let objetivo = if apagado { 0.0 } else { self.objetivo.max(base) };
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
            // Respiración: el borde se enciende y se apaga despacio (ciclo de 2,8 s),
            // y se engrosa un poco al inspirar. Al alcance de la mano, más marcado.
            let fase = (self.t * std::f32::consts::TAU / 2.8).cos();
            let aire = (0.5 - 0.5 * fase).powf(1.4);
            let nivel = self.nivel.min(1.0);
            let alfa = nivel * 0.62 * (0.22 + 0.78 * aire);
            let grosor = 0.8 + 1.4 * nivel + 1.1 * aire;
            mat.set_shader_parameter("alfa", &alfa.to_variant());
            mat.set_shader_parameter("grosor", &grosor.to_variant());
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
    manga: Option<Manga>,
    /// Esqueleto de la malla de la mano de Meta (lo que se ve) y su hueso de la muñeca.
    esqueleto: Option<Gd<Skeleton3D>>,
    hueso_muneca: i32,
    /// Por dónde se ubicó el brazo en el último cuadro (para el log de diagnóstico).
    fuente: &'static str,
}

/// Brazo visible: manga de camisa (brazo y antebrazo), codo, hombro y puño.
struct Manga {
    brazo: Gd<MeshInstance3D>,
    antebrazo: Gd<MeshInstance3D>,
    /// Mitad de la manga hacia el puño (la manga se ensancha y se frunce).
    antebrazo2: Gd<MeshInstance3D>,
    codo: Gd<MeshInstance3D>,
    hombro: Gd<MeshInstance3D>,
    puno: Gd<MeshInstance3D>,
}

impl Manga {
    fn mostrar(&mut self, si: bool) {
        for p in [
            &mut self.brazo,
            &mut self.antebrazo,
            &mut self.antebrazo2,
            &mut self.codo,
            &mut self.hombro,
            &mut self.puno,
        ] {
            if p.is_visible() != si {
                p.set_visible(si);
            }
        }
    }
}

/// Largo del brazo (hombro-codo) y del antebrazo (codo-muñeca), en metros.
const LARGO_BRAZO: f32 = 0.29;
const LARGO_ANTEBRAZO: f32 = 0.265;

fn pieza(nombre: &str, malla: Gd<Mesh>, mat: &Gd<StandardMaterial3D>) -> Gd<MeshInstance3D> {
    let mut m = MeshInstance3D::new_alloc();
    m.set_name(nombre);
    m.set_mesh(&malla);
    m.set_material_override(mat);
    m.set_cast_shadows_setting(ShadowCastingSetting::OFF);
    m.set_as_top_level(true);
    m.set_visible(false);
    m
}

fn cilindro(arriba: f32, abajo: f32) -> Gd<Mesh> {
    let mut c = CylinderMesh::new_gd();
    c.set_top_radius(arriba);
    c.set_bottom_radius(abajo);
    c.set_height(1.0);
    c.set_radial_segments(14);
    c.set_rings(1);
    c.upcast()
}

fn esfera(r: f32) -> Gd<Mesh> {
    let mut e = SphereMesh::new_gd();
    e.set_radius(r);
    e.set_height(2.0 * r);
    e.set_radial_segments(14);
    e.set_rings(7);
    e.upcast()
}

/// Ubica un cilindro de alto 1 (eje Y) entre `abajo` y `arriba`.
fn tramo(m: &mut Gd<MeshInstance3D>, abajo: Vector3, arriba: Vector3) {
    let v = arriba - abajo;
    let largo = v.length().max(1e-3);
    let y = v / largo;
    let ref_ = if y.y.abs() < 0.9 { Vector3::UP } else { Vector3::RIGHT };
    let x = y.cross(ref_).normalized();
    let z = x.cross(y);
    let b = Basis::from_cols(x, y * largo, z);
    m.set_global_transform(Transform3D::new(b, (abajo + arriba) * 0.5));
}

/// Codo por IK de dos huesos (si la mano queda más lejos que el largo del brazo,
/// el brazo se estira en vez de despegarse de la mano).
fn codo_ik(hombro: Vector3, muneca: Vector3, polo: Vector3, antebrazo: f32) -> Vector3 {
    let v = muneca - hombro;
    let d = v.length().max(1e-3);
    let dir = v / d;
    let (mut l1, mut l2) = (LARGO_BRAZO, antebrazo);
    if d > l1 + l2 {
        let k = d / (l1 + l2);
        l1 *= k;
        l2 *= k;
    }
    let a = ((l1 * l1 - l2 * l2 + d * d) / (2.0 * d)).clamp(-l1, l1);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let mut p = polo - dir * polo.dot(dir);
    if p.length() < 1e-3 {
        p = Vector3::DOWN;
    }
    hombro + dir * a + p.normalized() * h
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct ManosXR {
    manos: Vec<Mano>,
    cabeza: Option<Gd<XrCamera3D>>,
    /// Hacia dónde mira el torso (sigue a la cabeza con retraso).
    frente_cuerpo: Vector3,
    /// Brazos con manga (experimental, -- --brazos). En la PC, con muñecas simuladas
    /// delante de la cámara.
    prueba: bool,
    /// Cuello, frente y derecha del torso en este cuadro (de la cabeza).
    cuerpo: Option<(Vector3, Vector3, Vector3)>,
    diag_t: f32,
    diag_previo: String,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for ManosXR {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            manos: Vec::new(),
            cabeza: None,
            frente_cuerpo: Vector3::FORWARD,
            prueba: false,
            cuerpo: None,
            diag_t: 0.0,
            diag_previo: String::new(),
            base,
        }
    }

    fn ready(&mut self) {
        let Some(origen) = self.base().get_parent().and_then(|p| p.try_cast::<Node3D>().ok()) else {
            return;
        };
        self.cabeza = origen.try_get_node_as::<XrCamera3D>("Cabeza");
        self.prueba = argumento_usuario("brazos").is_some();
        // Camisa de lienzo crudo (la manga) y el puño un poco más gastado.
        // Arrugas de la tela: ruido estirado a lo largo de la manga como mapa de normales.
        let mut ruido = FastNoiseLite::new_gd();
        ruido.set_frequency(0.05);
        ruido.set_seed(7);
        let mut arrugas = NoiseTexture2D::new_gd();
        arrugas.set_width(128);
        arrugas.set_height(128);
        arrugas.set_seamless(true);
        arrugas.set_as_normal_map(true);
        arrugas.set_bump_strength(2.5);
        arrugas.set_noise(&ruido);
        let mut camisa = StandardMaterial3D::new_gd();
        camisa.set_albedo(Color::from_rgb(0.7, 0.65, 0.55));
        camisa.set_roughness(1.0);
        camisa.set_feature(Feature::NORMAL_MAPPING, true);
        camisa.set_texture(TextureParam::NORMAL, &arrugas);
        camisa.set_normal_scale(0.35);
        camisa.set_uv1_scale(Vector3::new(0.6, 2.5, 1.0));
        let mut puno = StandardMaterial3D::new_gd();
        puno.set_albedo(Color::from_rgb(0.6, 0.55, 0.45));
        puno.set_roughness(1.0);
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
                malla: malla.clone(),
                prev_pos: Vector3::ZERO,
                vel: Vector3::ZERO,
                vel_ang: Vector3::ZERO,
                prev_basis: Basis::IDENTITY,
                cerrada: false,
                tomado: None,
                caricia: 0.0,
                sorbo_cd: 0.0,
                esqueleto: malla.as_ref().and_then(|m| m.clone().try_cast::<Skeleton3D>().ok()),
                hueso_muneca: -1,
                fuente: "",
                manga: self.prueba.then(|| Manga {
                    brazo: pieza(&format!("Brazo_{lado}"), cilindro(0.056, 0.047), &camisa),
                    antebrazo: pieza(&format!("Antebrazo_{lado}"), cilindro(0.054, 0.047), &camisa),
                    antebrazo2: pieza(&format!("Antebrazo2_{lado}"), cilindro(0.038, 0.054), &camisa),
                    codo: pieza(&format!("Codo_{lado}"), esfera(0.048), &camisa),
                    hombro: pieza(&format!("Hombro_{lado}"), esfera(0.058), &camisa),
                    puno: pieza(&format!("Puno_{lado}"), cilindro(0.037, 0.037), &puno),
                }),
            });
        }
        let piezas: Vec<Gd<MeshInstance3D>> = self
            .manos
            .iter()
            .filter_map(|m| m.manga.as_ref())
            .flat_map(|g| {
                [
                    g.brazo.clone(),
                    g.antebrazo.clone(),
                    g.antebrazo2.clone(),
                    g.codo.clone(),
                    g.hombro.clone(),
                    g.puno.clone(),
                ]
            })
            .collect();
        for p in piezas {
            self.base_mut().add_child(&p);
        }
        // Cuando el esqueleto de cada mano termina de ponerse en pose (después de
        // XRHandModifier3D), el brazo se ubica en su muñeca: misma posición, sin atraso.
        let yo = self.to_gd();
        let metodos = if self.prueba { vec![(0, "mano_izq_actualizada"), (1, "mano_der_actualizada")] } else { Vec::new() };
        for (i, metodo) in metodos {
            if let Some(mut sk) = self.manos.get(i).and_then(|m| m.esqueleto.clone()) {
                sk.connect("skeleton_updated", &Callable::from_object_method(&yo, metodo));
            }
        }
    }

    fn process(&mut self, delta: f64) {
        if !self.prueba {
            return;
        }
        // El torso y los brazos que no siguen a una malla de mano (controles).
        self.actualizar_brazos(delta as f32);
        self.diagnostico(delta as f32);
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

#[godot_api]
impl ManosXR {
    #[func]
    fn mano_izq_actualizada(&mut self) {
        self.brazo_desde_malla(0);
    }

    #[func]
    fn mano_der_actualizada(&mut self) {
        self.brazo_desde_malla(1);
    }
}

/// Distancia (m) del guante: su centro está en el agarre del control y la
/// muñeca queda en su borde de atrás, sobre la línea del antebrazo.
const GUANTE_A_MUNECA: f32 = 0.075;

impl ManosXR {
    fn origen_xf(&self) -> Option<Transform3D> {
        let origen = self.base().get_parent().and_then(|p| p.try_cast::<Node3D>().ok())?;
        Some(origen.get_global_transform())
    }

    /// Muñeca y dirección de la mano según los datos del hand tracking (articulaciones).
    fn muneca_articulaciones(&self, i: usize, oxf: Transform3D) -> Option<(Vector3, Vector3)> {
        let m = &self.manos[i];
        let xr = XrServer::singleton();
        let t = xr.get_tracker(&m.tracker).and_then(|t| t.try_cast::<XrHandTracker>().ok())?;
        if !t.get_has_tracking_data() {
            return None;
        }
        let w = (oxf * t.get_hand_joint_transform(HandJoint::WRIST)).origin;
        let p = (oxf * t.get_hand_joint_transform(HandJoint::MIDDLE_FINGER_METACARPAL)).origin;
        let dir = if (p - w).length() > 1e-3 { (p - w).normalized() } else { Vector3::FORWARD };
        Some((w, dir))
    }

    /// Hueso de la muñeca en la malla de la mano (por nombre; si no, el más cercano
    /// a la articulación de la muñeca). Se busca una vez que el esqueleto tiene huesos.
    fn hueso_muneca(&mut self, i: usize, sk: &Gd<Skeleton3D>, oxf: Transform3D) -> i32 {
        if self.manos[i].hueso_muneca >= 0 {
            return self.manos[i].hueso_muneca;
        }
        let n = sk.get_bone_count();
        if n == 0 {
            return -1;
        }
        let mut elegido = -1;
        for b in 0..n {
            let nombre = sk.get_bone_name(b).to_string().to_lowercase();
            if nombre.contains("wrist") || nombre == "lefthand" || nombre == "righthand" || nombre.starts_with("hand") {
                elegido = b;
                break;
            }
        }
        if elegido < 0 {
            let Some((w, _)) = self.muneca_articulaciones(i, oxf) else { return -1 };
            let sxf = sk.get_global_transform();
            let mut mejor = f32::MAX;
            for b in 0..n {
                let d = (sxf * sk.get_bone_global_pose(b)).origin.distance_to(w);
                if d < mejor {
                    mejor = d;
                    elegido = b;
                }
            }
        }
        if elegido >= 0 {
            godot_print!(
                "Manos: {} usa el hueso '{}' ({} huesos) como muñeca",
                if i == 0 { "izquierda" } else { "derecha" },
                sk.get_bone_name(elegido),
                n
            );
        }
        self.manos[i].hueso_muneca = elegido;
        elegido
    }

    /// La malla de la mano ya está en pose: el brazo va a la muñeca que se ve.
    fn brazo_desde_malla(&mut self, i: usize) {
        let Some(sk) = self.manos.get(i).and_then(|m| m.esqueleto.clone()) else { return };
        if !sk.is_visible_in_tree() || self.cuerpo.is_none() {
            return;
        }
        let Some(oxf) = self.origen_xf() else { return };
        let b = self.hueso_muneca(i, &sk, oxf);
        if b < 0 {
            return;
        }
        let w = (sk.get_global_transform() * sk.get_bone_global_pose(b)).origin;
        let dir = self.muneca_articulaciones(i, oxf).map_or(Vector3::FORWARD, |(_, d)| d);
        self.ubicar_brazo(i, w, dir, None);
        self.manos[i].fuente = "malla";
    }

    /// Torso (desde la cabeza) y los brazos que no siguen a una malla de mano:
    /// con controles (guante) o, si la malla no tiene huesos, con las articulaciones.
    fn actualizar_brazos(&mut self, dt: f32) {
        let Some(cam) = self.cabeza.clone() else { return };
        let Some(oxf) = self.origen_xf() else { return };
        let cxf = cam.get_global_transform();
        let mut frente = -cxf.basis.col_c();
        frente.y = 0.0;
        if frente.length() > 0.2 {
            let f = frente.normalized();
            self.frente_cuerpo = self.frente_cuerpo.lerp(f, (dt * 2.5).min(1.0)).normalized();
        }
        let frente = self.frente_cuerpo;
        let derecha = frente.cross(Vector3::UP).normalized();
        let cuello = cxf.origin - Vector3::UP * 0.23 - frente * 0.08;
        self.cuerpo = Some((cuello, frente, derecha));
        for i in 0..self.manos.len() {
            let lado = if i == 0 { -1.0 } else { 1.0 };
            // Con la malla de la mano visible, la manga la ubica brazo_desde_malla.
            let con_malla = self.manos[i].esqueleto.as_ref().is_some_and(|s| s.is_visible_in_tree())
                && self.manos[i].hueso_muneca >= 0;
            if con_malla {
                continue;
            }
            let control = self.manos[i]
                .control
                .clone()
                .filter(|c| c.get_has_tracking_data() && self.muneca_articulaciones(i, oxf).is_none());
            if let Some(c) = control {
                let xf = c.get_global_transform();
                self.ubicar_brazo(i, xf.origin, -xf.basis.col_c().normalized(), Some(xf));
                self.manos[i].fuente = "control";
                continue;
            }
            if let Some((w, dir)) = self.muneca_articulaciones(i, oxf) {
                self.ubicar_brazo(i, w, dir, None);
                self.manos[i].fuente = "articulaciones";
                continue;
            }
            if self.prueba {
                // Prueba en la PC: manos delante, a la altura del pecho.
                let w = cxf.origin + frente * 0.42 + derecha * (lado * 0.2) - Vector3::UP * 0.2;
                self.ubicar_brazo(i, w, (frente - derecha * lado * 0.3).normalized(), None);
                self.manos[i].fuente = "prueba";
                continue;
            }
            if let Some(manga) = self.manos[i].manga.as_mut() {
                manga.mostrar(false);
            }
            self.manos[i].fuente = "nada";
        }
    }

    /// Hombro estimado, codo por IK y la manga hasta la muñeca `w`. Con control
    /// (`agarre`), `w` es el centro del agarre: el guante se alinea con el
    /// antebrazo y la muñeca queda en su borde de atrás.
    fn ubicar_brazo(&mut self, i: usize, w: Vector3, dir_mano: Vector3, agarre: Option<Transform3D>) {
        let Some((cuello, frente, derecha)) = self.cuerpo else { return };
        let lado = if i == 0 { -1.0 } else { 1.0 };
        let hombro = cuello - Vector3::UP * 0.04 + derecha * (lado * 0.18);
        let polo = Vector3::DOWN + derecha * (lado * 0.55) - frente * 0.35;
        let extra = if agarre.is_some() { GUANTE_A_MUNECA } else { 0.0 };
        let codo = codo_ik(hombro, w, polo, LARGO_ANTEBRAZO + extra);
        let mut dir_ante = w - codo;
        if dir_ante.length() < 1e-3 {
            dir_ante = dir_mano;
        }
        let dir_ante = dir_ante.normalized();
        let (w, dir_mano) = match agarre {
            Some(xf) => {
                // Guante alineado con el antebrazo, centrado en el agarre.
                if let Some(mut g) = self.manos[i].control.as_ref().and_then(|c| c.try_get_node_as::<Node3D>("Guante")) {
                    let z = dir_ante;
                    let arriba = xf.basis.col_b();
                    let mut x = arriba.cross(z);
                    if x.length() < 1e-3 {
                        x = Vector3::RIGHT;
                    }
                    let x = x.normalized();
                    let y = z.cross(x);
                    g.set_global_transform(Transform3D::new(Basis::from_cols(x, y, z), xf.origin));
                }
                (w - dir_ante * GUANTE_A_MUNECA, dir_ante)
            }
            None => (w, dir_mano),
        };
        let Some(manga) = self.manos[i].manga.as_mut() else { return };
        // El puño va en la muñeca, alineado entre el antebrazo y la mano.
        let eje_puno = (dir_ante * 0.7 + dir_mano * 0.3).normalized();
        let fin_manga = w - dir_ante * 0.035;
        tramo(&mut manga.brazo, codo, hombro);
        // Manga ancha a media altura del antebrazo, fruncida hacia el puño.
        let medio = codo.lerp(fin_manga, 0.55);
        tramo(&mut manga.antebrazo, medio, codo);
        tramo(&mut manga.antebrazo2, fin_manga, medio);
        tramo(&mut manga.puno, w + eje_puno * 0.012, w - eje_puno * 0.045);
        manga.codo.set_global_position(codo);
        manga.hombro.set_global_position(hombro);
        manga.mostrar(true);
    }

    /// Log (logcat) cuando cambia de dónde sale cada brazo, y cuánto se separa la
    /// muñeca de la malla de la articulación del hand tracking.
    fn diagnostico(&mut self, dt: f32) {
        self.diag_t += dt;
        if self.diag_t < 1.0 {
            return;
        }
        self.diag_t = 0.0;
        let Some(oxf) = self.origen_xf() else { return };
        let mut partes = Vec::new();
        for i in 0..self.manos.len() {
            let mut txt = format!("{}={}", if i == 0 { "izq" } else { "der" }, self.manos[i].fuente);
            if let (Some(sk), Some((w, _))) = (self.manos[i].esqueleto.clone(), self.muneca_articulaciones(i, oxf)) {
                let b = self.manos[i].hueso_muneca;
                if b >= 0 && sk.is_visible_in_tree() {
                    let d = (sk.get_global_transform() * sk.get_bone_global_pose(b)).origin.distance_to(w);
                    txt += &format!(" (malla vs articulación: {:.0} cm)", d * 100.0);
                }
            }
            partes.push(txt);
        }
        let estado = partes.join(", ");
        if estado != self.diag_previo {
            godot_print!("Brazos: {estado}");
            self.diag_previo = estado;
        }
    }

    /// Contorno amarillo: suave si estás cerca, fuerte si la mano lo alcanza.
    fn resaltar_interactivos(&mut self, cabeza: Option<Vector3>, oxf: Transform3D) {
        if cabeza.is_none() {
            return;
        }
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
            let nivel = if palmas.iter().any(|m| m.distance_to(p) < r) { 1.0 } else { 0.0 };
            a.bind_mut().resaltar(nivel);
        }
        for n in arbol.get_nodes_in_group("acariciable").iter_shared() {
            let Ok(mut an) = n.try_cast::<Animal>() else { continue };
            let (p, r) = (an.bind().punto_global(), an.bind().radio());
            let nivel = if palmas.iter().any(|m| m.distance_to(p) < r) { 1.0 } else { 0.0 };
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
