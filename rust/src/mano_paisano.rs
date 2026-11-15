//! Mano del jugador hecha por código: mano con dedos y antebrazo con manga de
//! camisa de paisano, en una sola malla con un solo esqueleto. Los huesos llevan
//! los nombres que busca `XRHandModifier3D` ("Left<Hueso>"/"Right<Hueso>") y la
//! convención humanoide de Godot (+Y a lo largo del hueso hacia la punta, +Z
//! hacia la palma), que es como Godot entrega las articulaciones de OpenXR. El
//! espacio del esqueleto es el de la palma (la pose "default" del tracker).
//!
//! El modificador mueve la muñeca y los dedos con el hand tracking (solo
//! rotaciones: las proporciones son las del modelo). La manga cuelga de un hueso
//! extra, hijo de la muñeca, que `Antebrazo` apunta hacia el codo estimado: así
//! la manga nace siempre pegada a la mano.

use std::f32::consts::TAU;

use godot::classes::{
    mesh::{ArrayType, PrimitiveType},
    xr_hand_modifier_3d::BoneUpdate,
    xr_hand_tracker::{HandJoint, HandJointFlags},
    xr_pose::TrackingConfidence,
    xr_positional_tracker::TrackerHand,
    ArrayMesh, ISkeletonModifier3D, Material, MeshInstance3D, Skeleton3D, SkeletonModifier3D, Skin,
    XrCamera3D, XrHandModifier3D, XrHandTracker, XrServer,
};
use godot::prelude::*;

/// Largo del brazo (hombro-codo) y del antebrazo (codo-muñeca), en metros.
const LARGO_BRAZO: f32 = 0.29;
const LARGO_ANTEBRAZO: f32 = 0.265;
/// Lo más que la manga se aparta del eje de la mano (grados).
const DESVIO_MAXIMO: f32 = 50.0;

/// Índices de los huesos (0..25 en el orden de XRHandTracker, salvo que acá la
/// muñeca va primero porque es la raíz) y el antebrazo.
const MUNECA: usize = 0;
const PALMA: usize = 1;
const ANTEBRAZO: usize = 26;
const NOMBRES: [&str; 27] = [
    "Hand", "Palm",
    "ThumbMetacarpal", "ThumbProximal", "ThumbDistal", "ThumbTip",
    "IndexMetacarpal", "IndexProximal", "IndexIntermediate", "IndexDistal", "IndexTip",
    "MiddleMetacarpal", "MiddleProximal", "MiddleIntermediate", "MiddleDistal", "MiddleTip",
    "RingMetacarpal", "RingProximal", "RingIntermediate", "RingDistal", "RingTip",
    "LittleMetacarpal", "LittleProximal", "LittleIntermediate", "LittleDistal", "LittleTip",
    "Antebrazo",
];

// ---------------------------------------------------------------- forma
// Medidas en el marco "anatómico" de la mano izquierda con la palma hacia abajo:
// +Y = dorso, -Z = hacia los dedos, +X = lado del pulgar. La palma está en el origen.

struct Dedo {
    metacarpo: Vector3,
    nudillo: Vector3,
    dir: Vector3,
    largos: [f32; 3],
    radio: f32,
}

fn dedos() -> [Dedo; 4] {
    let d = |x: f32| Vector3::new(x, -0.02, -1.0).normalized();
    [
        Dedo { metacarpo: Vector3::new(0.012, 0.002, 0.036), nudillo: Vector3::new(0.024, 0.004, -0.042), dir: d(0.08), largos: [0.039, 0.024, 0.022], radio: 0.0090 },
        Dedo { metacarpo: Vector3::new(0.002, 0.003, 0.038), nudillo: Vector3::new(0.004, 0.005, -0.046), dir: d(0.0), largos: [0.044, 0.027, 0.023], radio: 0.0094 },
        Dedo { metacarpo: Vector3::new(-0.008, 0.002, 0.036), nudillo: Vector3::new(-0.014, 0.004, -0.043), dir: d(-0.07), largos: [0.041, 0.026, 0.022], radio: 0.0088 },
        Dedo { metacarpo: Vector3::new(-0.018, 0.0, 0.033), nudillo: Vector3::new(-0.030, 0.001, -0.036), dir: d(-0.15), largos: [0.032, 0.020, 0.019], radio: 0.0076 },
    ]
}

const POS_MUNECA: Vector3 = Vector3::new(0.0, 0.0, 0.045);
/// Pulgar: metacarpo, proximal, distal, punta.
const PULGAR: [Vector3; 4] = [
    Vector3::new(0.020, -0.008, 0.030),
    Vector3::new(0.043, -0.013, -0.002),
    Vector3::new(0.058, -0.015, -0.030),
    Vector3::new(0.068, -0.016, -0.054),
];

/// Marco anatómico (de la mano izquierda) -> espacio del esqueleto (convención de
/// Godot: X = Y × Z, Y hacia los dedos, Z hacia la palma). La derecha se espeja.
struct Lado {
    signo: f32,
}

impl Lado {
    fn a_esqueleto(&self, p: Vector3) -> Vector3 {
        Vector3::new(-p.x * self.signo, -p.z, -p.y)
    }
}

/// Transformación de un hueso: en `origen`, +Y hacia `hacia`, +Z cerca de `palmar`.
fn marco(origen: Vector3, hacia: Vector3, palmar: Vector3) -> Transform3D {
    let y = (hacia - origen).normalized();
    let z = (palmar - y * y.dot(palmar)).normalized();
    let x = y.cross(z);
    Transform3D::new(Basis::from_cols(x, y, z), origen)
}

/// Padre y transformación global (espacio del esqueleto) de cada hueso.
fn huesos(lado: &Lado) -> Vec<(Option<usize>, Transform3D)> {
    let s = |p: Vector3| lado.a_esqueleto(p);
    let palmar = Vector3::new(0.0, 0.0, 1.0);
    // La yema del pulgar mira hacia el índice y hacia la palma.
    let palmar_pulgar = s(Vector3::new(-0.7, -0.7, 0.0)).normalized();
    let mut v: Vec<(Option<usize>, Transform3D)> = vec![
        (None, Transform3D::new(Basis::IDENTITY, s(POS_MUNECA))),
        (Some(MUNECA), Transform3D::new(Basis::IDENTITY, Vector3::ZERO)),
    ];
    let p = PULGAR.map(s);
    v.push((Some(MUNECA), marco(p[0], p[1], palmar_pulgar)));
    v.push((Some(2), marco(p[1], p[2], palmar_pulgar)));
    v.push((Some(3), marco(p[2], p[3], palmar_pulgar)));
    let fin = marco(p[2], p[3], palmar_pulgar);
    v.push((Some(4), Transform3D::new(fin.basis, p[3])));
    for d in dedos() {
        let b = v.len();
        let nud = d.nudillo;
        let inter = nud + d.dir * d.largos[0];
        let dist = inter + d.dir * d.largos[1];
        let punta = dist + d.dir * d.largos[2];
        let [m, n, i, di, pu] = [d.metacarpo, nud, inter, dist, punta].map(s);
        v.push((Some(MUNECA), marco(m, n, palmar)));
        v.push((Some(b), marco(n, i, palmar)));
        v.push((Some(b + 1), marco(i, di, palmar)));
        v.push((Some(b + 2), marco(di, pu, palmar)));
        v.push((Some(b + 3), Transform3D::new(marco(di, pu, palmar).basis, pu)));
    }
    // El antebrazo nace en la muñeca, alineado con ella (la manga va hacia -Y).
    v.push((Some(MUNECA), Transform3D::new(Basis::IDENTITY, s(POS_MUNECA))));
    v
}

// ---------------------------------------------------------------- malla

struct Seccion {
    c: Vector3,
    rx: f32,
    ry: f32,
    pesos: Vec<(usize, f32)>,
}

fn sec(c: Vector3, rx: f32, ry: f32, pesos: &[(usize, f32)]) -> Seccion {
    Seccion { c, rx, ry, pesos: pesos.to_vec() }
}

#[derive(Default)]
struct Malla {
    v: Vec<Vector3>,
    n: Vec<Vector3>,
    huesos: Vec<i32>,
    pesos: Vec<f32>,
    indices: Vec<i32>,
}

impl Malla {
    fn vertice(&mut self, p: Vector3, n: Vector3, pesos: &[(usize, f32)]) -> i32 {
        let total: f32 = pesos.iter().map(|(_, w)| w).sum::<f32>().max(1e-6);
        for k in 0..4 {
            let (h, w) = pesos.get(k).copied().unwrap_or((0, 0.0));
            self.huesos.push(h as i32);
            self.pesos.push(w / total);
        }
        self.v.push(p);
        self.n.push(n);
        (self.v.len() - 1) as i32
    }

    /// Tubo de sección elíptica por las secciones (rx de lado, ry de dorso a
    /// palma), con las puntas cerradas y redondeadas.
    fn tubo(&mut self, s: &[Seccion], arriba: Vector3, lados: usize) {
        let n = s.len();
        let tangente = |k: usize| {
            let a = s[k.saturating_sub(1)].c;
            let b = s[(k + 1).min(n - 1)].c;
            (b - a).normalized()
        };
        let mut anillos: Vec<Vec<i32>> = Vec::new();
        for (k, sk) in s.iter().enumerate() {
            let t = tangente(k);
            let u = (arriba - t * t.dot(arriba)).normalized();
            let v = t.cross(u);
            let anillo = (0..lados)
                .map(|j| {
                    let a = TAU * j as f32 / lados as f32;
                    let (c, si) = (a.cos(), a.sin());
                    let p = sk.c + v * (sk.rx * c) + u * (sk.ry * si);
                    let nor = (v * (c / sk.rx) + u * (si / sk.ry)).normalized();
                    self.vertice(p, nor, &sk.pesos)
                })
                .collect();
            anillos.push(anillo);
        }
        for k in 0..n - 1 {
            for j in 0..lados {
                let j1 = (j + 1) % lados;
                let (a, b) = (anillos[k][j], anillos[k][j1]);
                let (c, d) = (anillos[k + 1][j], anillos[k + 1][j1]);
                self.indices.extend([a, b, c, b, d, c]);
            }
        }
        // Tapas: un vértice central un poco hacia afuera.
        for (k, signo) in [(n - 1, 1.0f32), (0, -1.0)] {
            let t = tangente(k) * signo;
            let sk = &s[k];
            let apice = self.vertice(sk.c + t * sk.rx.min(sk.ry) * 0.9, t, &sk.pesos);
            for j in 0..lados {
                let j1 = (j + 1) % lados;
                let (a, b) = (anillos[k][j], anillos[k][j1]);
                if signo > 0.0 {
                    self.indices.extend([a, b, apice]);
                } else {
                    self.indices.extend([b, a, apice]);
                }
            }
        }
    }

    /// Pasa al espacio del esqueleto (espejando la derecha) y arma la superficie.
    fn superficie(&self, lado: &Lado, malla: &mut Gd<ArrayMesh>, material: &Gd<Material>) {
        let v: PackedVector3Array = self.v.iter().map(|p| lado.a_esqueleto(*p)).collect();
        let n: PackedVector3Array = self.n.iter().map(|p| lado.a_esqueleto(*p).normalized()).collect();
        let mut indices = self.indices.clone();
        if lado.signo < 0.0 {
            // Espejar invierte el sentido de los triángulos.
            for t in indices.chunks_mut(3) {
                t.swap(1, 2);
            }
        }
        let mut arrays = VarArray::new();
        arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
        arrays.set(ArrayType::VERTEX.ord() as usize, &v.to_variant());
        arrays.set(ArrayType::NORMAL.ord() as usize, &n.to_variant());
        arrays.set(ArrayType::BONES.ord() as usize, &PackedInt32Array::from(self.huesos.as_slice()).to_variant());
        arrays.set(ArrayType::WEIGHTS.ord() as usize, &PackedFloat32Array::from(self.pesos.as_slice()).to_variant());
        arrays.set(ArrayType::INDEX.ord() as usize, &PackedInt32Array::from(indices.as_slice()).to_variant());
        malla.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
        let i = malla.get_surface_count() - 1;
        malla.surface_set_material(i, material);
    }
}

/// Piel: palma, dedos, pulgar y muñeca. Manga: puño y manga hasta el codo.
fn formas() -> (Malla, Malla) {
    let mut piel = Malla::default();
    let arriba = Vector3::new(0.0, 1.0, 0.0);
    // Palma (de la muñeca a los nudillos).
    piel.tubo(
        &[
            sec(Vector3::new(0.0, -0.001, 0.052), 0.025, 0.017, &[(MUNECA, 1.0)]),
            sec(Vector3::new(-0.001, 0.0, 0.032), 0.032, 0.0175, &[(MUNECA, 0.5), (PALMA, 0.5)]),
            sec(Vector3::new(-0.002, 0.001, 0.008), 0.039, 0.0165, &[(PALMA, 1.0)]),
            sec(Vector3::new(-0.003, 0.002, -0.020), 0.0425, 0.0155, &[(PALMA, 1.0)]),
            sec(Vector3::new(-0.003, 0.003, -0.040), 0.041, 0.0145, &[(PALMA, 1.0)]),
        ],
        arriba,
        16,
    );
    // Dedos.
    for (f, d) in dedos().iter().enumerate() {
        let b = 6 + 5 * f;
        let (l1, l2, l3) = (d.largos[0], d.largos[1], d.largos[2]);
        let inter = d.nudillo + d.dir * l1;
        let dist = inter + d.dir * l2;
        let punta = dist + d.dir * l3;
        let r = d.radio;
        let s = |c: Vector3, k: f32, p: &[(usize, f32)]| sec(c, r * k * 1.08, r * k * 0.9, p);
        piel.tubo(
            &[
                s(d.nudillo - d.dir * 0.012, 1.05, &[(b, 1.0)]),
                s(d.nudillo, 1.0, &[(b, 0.5), (b + 1, 0.5)]),
                s(d.nudillo + d.dir * (l1 * 0.5), 0.97, &[(b + 1, 1.0)]),
                s(inter, 0.93, &[(b + 1, 0.5), (b + 2, 0.5)]),
                s(inter + d.dir * (l2 * 0.5), 0.9, &[(b + 2, 1.0)]),
                s(dist, 0.87, &[(b + 2, 0.5), (b + 3, 0.5)]),
                s(dist + d.dir * (l3 * 0.5), 0.84, &[(b + 3, 1.0)]),
                s(punta - d.dir * (r * 0.75), 0.8, &[(b + 3, 1.0)]),
            ],
            arriba,
            12,
        );
    }
    // Pulgar.
    let [m, p, d, t] = PULGAR;
    let dir = (t - d).normalized();
    piel.tubo(
        &[
            sec(m.lerp(p, 0.15), 0.0135, 0.0125, &[(MUNECA, 0.4), (2, 0.6)]),
            sec(m.lerp(p, 0.6), 0.0125, 0.0115, &[(2, 1.0)]),
            sec(p, 0.0112, 0.0105, &[(2, 0.5), (3, 0.5)]),
            sec(p.lerp(d, 0.5), 0.0106, 0.0099, &[(3, 1.0)]),
            sec(d, 0.0100, 0.0094, &[(3, 0.5), (4, 0.5)]),
            sec(d.lerp(t, 0.5), 0.0095, 0.0089, &[(4, 1.0)]),
            sec(t - dir * 0.007, 0.0088, 0.0082, &[(4, 1.0)]),
        ],
        arriba,
        12,
    );
    // Muñeca (queda dentro del puño).
    piel.tubo(
        &[
            sec(Vector3::new(0.0, -0.001, 0.040), 0.026, 0.018, &[(MUNECA, 1.0)]),
            sec(Vector3::new(0.0, -0.001, 0.060), 0.0255, 0.0185, &[(MUNECA, 0.6), (ANTEBRAZO, 0.4)]),
            sec(Vector3::new(0.0, -0.001, 0.085), 0.026, 0.019, &[(ANTEBRAZO, 1.0)]),
        ],
        arriba,
        16,
    );
    // Manga de camisa de paisano: puño ajustado y la manga ancha hacia el codo.
    let mut manga = Malla::default();
    manga.tubo(
        &[
            sec(Vector3::new(0.0, 0.0, 0.072), 0.0265, 0.0195, &[(MUNECA, 0.5), (ANTEBRAZO, 0.5)]),
            sec(Vector3::new(0.0, 0.0, 0.078), 0.034, 0.031, &[(MUNECA, 0.35), (ANTEBRAZO, 0.65)]),
            sec(Vector3::new(0.0, 0.0, 0.112), 0.036, 0.033, &[(ANTEBRAZO, 1.0)]),
            sec(Vector3::new(0.0, 0.0, 0.118), 0.0405, 0.0375, &[(ANTEBRAZO, 1.0)]),
            sec(Vector3::new(0.0, 0.002, 0.19), 0.047, 0.044, &[(ANTEBRAZO, 1.0)]),
            sec(Vector3::new(0.0, 0.003, 0.27), 0.050, 0.047, &[(ANTEBRAZO, 1.0)]),
            sec(Vector3::new(0.0, 0.003, 0.32), 0.048, 0.045, &[(ANTEBRAZO, 1.0)]),
        ],
        arriba,
        16,
    );
    (piel, manga)
}

/// Esqueleto con la malla de la mano (izquierda o derecha) y su piel.
pub fn crear_mano(izquierda: bool, piel: &Gd<Material>, camisa: &Gd<Material>) -> Gd<Skeleton3D> {
    let lado = Lado { signo: if izquierda { 1.0 } else { -1.0 } };
    let prefijo = if izquierda { "Left" } else { "Right" };
    let datos = huesos(&lado);
    let mut sk = Skeleton3D::new_alloc();
    sk.set_name(&format!("ManoPaisano_{prefijo}"));
    let mut piel_skin = Skin::new_gd();
    for (i, (padre, global)) in datos.iter().enumerate() {
        sk.add_bone(&format!("{prefijo}{}", NOMBRES[i]));
        let local = match padre {
            Some(p) => {
                sk.set_bone_parent(i as i32, *p as i32);
                datos[*p].1.affine_inverse() * *global
            }
            None => *global,
        };
        sk.set_bone_rest(i as i32, local);
        sk.set_bone_pose(i as i32, local);
        piel_skin.add_bind(i as i32, global.affine_inverse());
    }
    let (forma_piel, forma_manga) = formas();
    let mut malla = ArrayMesh::new_gd();
    forma_piel.superficie(&lado, &mut malla, piel);
    forma_manga.superficie(&lado, &mut malla, camisa);
    let mut mi = MeshInstance3D::new_alloc();
    mi.set_name("Malla");
    mi.set_mesh(&malla);
    mi.set_skin(&piel_skin);
    mi.set_cast_shadows_setting(godot::classes::geometry_instance_3d::ShadowCastingSetting::OFF);
    sk.add_child(&mi);
    mi.set_skeleton_path(&NodePath::from(".."));
    sk
}

/// Agrega a `sk` el modificador del hand tracking (solo rotaciones) y el que
/// orienta la manga.
pub fn conectar(sk: &mut Gd<Skeleton3D>, tracker: &StringName, izquierda: bool, camara: Option<Gd<XrCamera3D>>) {
    let mut modif = XrHandModifier3D::new_alloc();
    modif.set_hand_tracker(tracker);
    modif.set_bone_update(BoneUpdate::ROTATION_ONLY);
    sk.add_child(&modif);
    let mut ante = Antebrazo::new_alloc();
    {
        let mut a = ante.bind_mut();
        a.prefijo = if izquierda { "Left" } else { "Right" }.to_string();
        a.lado = if izquierda { -1.0 } else { 1.0 };
        a.camara = camara;
    }
    sk.add_child(&ante);
}

// ---------------------------------------------------------------- antebrazo

/// Codo por IK de dos huesos (si la mano queda más lejos que el brazo, se estira).
fn codo_ik(hombro: Vector3, muneca: Vector3, polo: Vector3) -> Vector3 {
    let v = muneca - hombro;
    let d = v.length().max(1e-3);
    let dir = v / d;
    let (mut l1, mut l2) = (LARGO_BRAZO, LARGO_ANTEBRAZO);
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

/// Rotación más corta que lleva `a` a `b` (vectores unitarios).
fn rotacion_entre(a: Vector3, b: Vector3) -> Quaternion {
    let eje = a.cross(b);
    if eje.length() < 1e-5 {
        return Quaternion::IDENTITY;
    }
    Quaternion::from_axis_angle(eje.normalized(), a.angle_to(b))
}

/// Apunta el hueso del antebrazo (la manga) hacia el codo estimado desde la
/// cabeza, sin apartarse más de DESVIO_MAXIMO del eje de la mano.
#[derive(GodotClass)]
#[class(base = SkeletonModifier3D)]
pub struct Antebrazo {
    prefijo: String,
    lado: f32,
    camara: Option<Gd<XrCamera3D>>,
    frente: Vector3,
    indices: Option<(i32, i32)>,
    base: Base<SkeletonModifier3D>,
}

#[godot_api]
impl ISkeletonModifier3D for Antebrazo {
    fn init(base: Base<SkeletonModifier3D>) -> Self {
        Self { prefijo: String::new(), lado: 1.0, camara: None, frente: Vector3::FORWARD, indices: None, base }
    }

    fn process_modification_with_delta(&mut self, delta: f64) {
        let Some(mut sk) = self.base().get_skeleton() else { return };
        if self.indices.is_none() {
            let m = sk.find_bone(&format!("{}Hand", self.prefijo));
            let a = sk.find_bone(&format!("{}Antebrazo", self.prefijo));
            if m < 0 || a < 0 {
                return;
            }
            self.indices = Some((m, a));
        }
        let Some((i_mano, i_ante)) = self.indices else { return };
        let sxf = sk.get_global_transform();
        let mano_s = sk.get_bone_global_pose(i_mano);
        let mano_g = sxf * mano_s;
        let atras = -mano_g.basis.col_b().normalized();
        let mut objetivo = atras;
        if let Some(cam) = self.camara.as_ref().filter(|c| c.is_inside_tree()) {
            let cxf = cam.get_global_transform();
            let mut f = -cxf.basis.col_c();
            f.y = 0.0;
            if f.length() > 0.2 {
                let k = (delta as f32 * 2.5).min(1.0);
                self.frente = self.frente.lerp(f.normalized(), k).normalized();
            }
            let derecha = self.frente.cross(Vector3::UP).normalized();
            let hombro = cxf.origin - Vector3::UP * 0.27 - self.frente * 0.08 + derecha * (self.lado * 0.18);
            let polo = Vector3::DOWN + derecha * (self.lado * 0.55) - self.frente * 0.35;
            let codo = codo_ik(hombro, mano_g.origin, polo);
            let hacia_codo = codo - mano_g.origin;
            if hacia_codo.length() > 1e-3 {
                let d = hacia_codo.normalized();
                let maximo = DESVIO_MAXIMO.to_radians();
                let ang = atras.angle_to(d);
                objetivo = if ang <= maximo {
                    d
                } else {
                    let eje = atras.cross(d);
                    if eje.length() < 1e-5 {
                        atras
                    } else {
                        Basis::from_axis_angle(eje.normalized(), maximo) * atras
                    }
                };
            }
        }
        // Giro global de "atrás" al objetivo, pasado al espacio local del hueso.
        let q_global = rotacion_entre(atras, objetivo);
        let r = sxf.basis.orthonormalized();
        let q_esqueleto = r.inverse() * Basis::from_quaternion(q_global) * r;
        let local = mano_s.basis.inverse() * (q_esqueleto * mano_s.basis);
        sk.set_bone_pose_rotation(i_ante, local.orthonormalized().get_quaternion());
    }
}

// ---------------------------------------------------------------- prueba en la PC

/// Tracker de mano falso (para ver la mano en la PC con `-- --manos`): pose de
/// las articulaciones a partir del esqueleto del modelo, con los dedos doblados
/// `curva` grados en cada articulación. `palma` va en el espacio del XROrigin3D.
pub fn actualizar_tracker_prueba(tracker: &mut Gd<XrHandTracker>, izquierda: bool, palma: Transform3D, curva: f32) {
    let lado = Lado { signo: if izquierda { 1.0 } else { -1.0 } };
    let datos = huesos(&lado);
    let mut global: Vec<Transform3D> = Vec::with_capacity(datos.len());
    for (i, (padre, g)) in datos.iter().enumerate() {
        let t = match padre {
            Some(p) => {
                let local = datos[*p].1.affine_inverse() * *g;
                // Doblar los dedos (no los metacarpos) alrededor de su X local.
                let doblar = !matches!(i, 0..=2 | 6 | 11 | 16 | 21 | 26);
                let giro = if doblar { Basis::from_axis_angle(Vector3::RIGHT, curva.to_radians()) } else { Basis::IDENTITY };
                global[*p] * Transform3D::new(local.basis * giro, local.origin)
            }
            None => *g,
        };
        global.push(t);
    }
    let banderas = HandJointFlags::ORIENTATION_VALID
        | HandJointFlags::ORIENTATION_TRACKED
        | HandJointFlags::POSITION_VALID
        | HandJointFlags::POSITION_TRACKED;
    for j in 0..26usize {
        // XRHandTracker: 0 = palma, 1 = muñeca; acá la muñeca es el hueso 0.
        let hueso = match j {
            0 => PALMA,
            1 => MUNECA,
            k => k,
        };
        let Some(junta) = HandJoint::try_from_ord(j as i32) else { continue };
        tracker.set_hand_joint_transform(junta, palma * global[hueso]);
        tracker.set_hand_joint_flags(junta, banderas);
    }
    tracker.set_has_tracking_data(true);
    tracker.set_pose("default", palma, Vector3::ZERO, Vector3::ZERO, TrackingConfidence::HIGH);
}

/// Crea y registra los trackers falsos de las dos manos.
pub fn registrar_trackers_prueba() -> [Gd<XrHandTracker>; 2] {
    let mut xr = XrServer::singleton();
    ["left", "right"].map(|lado| {
        let mut t = XrHandTracker::new_gd();
        t.set_tracker_name(&format!("/user/hand_tracker/{lado}"));
        t.set_tracker_hand(if lado == "left" { TrackerHand::LEFT } else { TrackerHand::RIGHT });
        xr.add_tracker(&t);
        t
    })
}
