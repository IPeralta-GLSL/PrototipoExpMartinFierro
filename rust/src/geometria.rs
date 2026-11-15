//! Geometría procedural pura (sin tipos de Godot): relieve de la pampa,
//! malla polar del terreno y matas de pasto / cortadera.
//!
//! Convención de Godot: las caras frontales se enrollan en sentido horario
//! vistas desde el frente.

use crate::azar::Azar;
use crate::ruido::Perlin2;

#[derive(Default)]
pub struct MallaDatos {
    pub vertices: Vec<[f32; 3]>,
    pub normales: Vec<[f32; 3]>,
    pub colores: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<i32>,
}

#[inline]
fn suave(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn mezclar3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
fn normalizar(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// Distancia de un punto a un segmento en el plano XZ.
fn dist_segmento(px: f32, pz: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, abz) = (b.0 - a.0, b.1 - a.1);
    let (apx, apz) = (px - a.0, pz - a.1);
    let l2 = abx * abx + abz * abz;
    let t = if l2 > 0.0 {
        ((apx * abx + apz * abz) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dz) = (apx - abx * t, apz - abz * t);
    (dx * dx + dz * dz).sqrt()
}

/// Descripción del relieve y del "suelo" (pasto / tierra pisada).
pub struct Relieve {
    perlin: Perlin2,
    detalle: Perlin2,
    color: Perlin2,
    pub amplitud: f32,
    pub escala: f32,
    pub radio_llano: f32,
    base: f32,
    /// Manchas de tierra pisada: (x, z, radio).
    pub manchas: Vec<(f32, f32, f32)>,
    /// Senderos: segmentos ((x0, z0), (x1, z1), ancho).
    pub senderos: Vec<((f32, f32), (f32, f32), f32)>,
}

impl Relieve {
    pub fn new(semilla: u64, amplitud: f32, escala: f32, radio_llano: f32) -> Self {
        let perlin = Perlin2::new(semilla);
        let base = perlin.fbm(0.0, 0.0, 4) * amplitud * 0.12;
        Self {
            perlin,
            detalle: Perlin2::new(semilla.wrapping_add(101)),
            color: Perlin2::new(semilla.wrapping_add(202)),
            amplitud,
            escala: escala.max(1.0),
            radio_llano,
            base,
            manchas: Vec::new(),
            senderos: Vec::new(),
        }
    }

    /// Altura del suelo en (x, z). El campamento (radio_llano) queda casi plano.
    pub fn altura(&self, x: f32, z: f32) -> f32 {
        let r = (x * x + z * z).sqrt();
        let llano = suave(self.radio_llano, self.radio_llano + 30.0, r);
        let ond = self.perlin.fbm(x / self.escala, z / self.escala, 4) * self.amplitud;
        let micro = self.detalle.ruido(x * 0.31, z * 0.31) * 0.05;
        ond * (0.12 + 0.88 * llano) - self.base + micro
    }

    pub fn normal(&self, x: f32, z: f32) -> [f32; 3] {
        let e = 0.35;
        let dx = self.altura(x + e, z) - self.altura(x - e, z);
        let dz = self.altura(x, z + e) - self.altura(x, z - e);
        normalizar([-dx, 2.0 * e, -dz])
    }

    /// 1 = pasto pleno, 0 = tierra pisada (fogón, senderos).
    pub fn cobertura_pasto(&self, x: f32, z: f32) -> f32 {
        let borde = self.detalle.ruido(x * 0.9, z * 0.9) * 0.35;
        let mut tierra: f32 = 0.0;
        for &(mx, mz, r) in &self.manchas {
            let d = ((x - mx).powi(2) + (z - mz).powi(2)).sqrt() + borde;
            tierra = tierra.max(1.0 - suave(r * 0.6, r, d));
        }
        for &(a, b, ancho) in &self.senderos {
            let d = dist_segmento(x, z, a, b) + borde * 0.6;
            tierra = tierra.max(1.0 - suave(ancho * 0.35, ancho * 0.5, d));
        }
        1.0 - tierra
    }

    /// Color del suelo (rgb) y cobertura de pasto (a).
    pub fn color_suelo(&self, x: f32, z: f32) -> [f32; 4] {
        let verde = [0.33, 0.40, 0.17];
        let dorado = [0.62, 0.54, 0.27];
        let seco = [0.50, 0.46, 0.30];
        let tierra = [0.36, 0.28, 0.19];
        let t1 = 0.5 + 0.5 * self.color.fbm(x / 38.0, z / 38.0, 3);
        let t2 = 0.5 + 0.5 * self.color.ruido(x / 9.0 + 50.0, z / 9.0 - 30.0);
        let mut c = mezclar3(verde, dorado, suave(0.25, 0.8, t1));
        c = mezclar3(c, seco, 0.35 * t2);
        let cob = self.cobertura_pasto(x, z);
        c = mezclar3(tierra, c, cob);
        [c[0], c[1], c[2], cob]
    }
}

/// Malla polar: densa cerca del jugador y cada vez más rala hacia el
/// horizonte. Un solo draw call para todo el suelo.
pub fn malla_terreno(rel: &Relieve, radio_max: f32, paso_inicial: f32, crecimiento: f32, segmentos: u32) -> MallaDatos {
    let mut m = MallaDatos::default();
    let seg = segmentos.max(8) as usize;

    let mut radios = Vec::new();
    let mut r = 0.0f32;
    let mut paso = paso_inicial.max(0.05);
    while r < radio_max {
        r += paso;
        radios.push(r.min(radio_max));
        paso *= crecimiento.max(1.0001);
    }

    let agregar = |x: f32, z: f32, m: &mut MallaDatos| {
        let y = rel.altura(x, z);
        m.vertices.push([x, y, z]);
        m.normales.push(rel.normal(x, z));
        m.colores.push(rel.color_suelo(x, z));
        m.uvs.push([x * 0.25, z * 0.25]);
    };

    agregar(0.0, 0.0, &mut m);
    for &rr in &radios {
        for j in 0..seg {
            let th = j as f32 / seg as f32 * std::f32::consts::TAU;
            agregar(rr * th.cos(), rr * th.sin(), &mut m);
        }
    }

    let idx = |anillo: usize, j: usize| -> i32 { (1 + anillo * seg + (j % seg)) as i32 };
    // abanico central
    for j in 0..seg {
        m.indices.extend_from_slice(&[0, idx(0, j), idx(0, j + 1)]);
    }
    // anillos
    for a in 0..radios.len().saturating_sub(1) {
        for j in 0..seg {
            let ia = idx(a, j);
            let ib = idx(a, j + 1);
            let ic = idx(a + 1, j);
            let id = idx(a + 1, j + 1);
            m.indices.extend_from_slice(&[ia, ic, ib, ib, ic, id]);
        }
    }
    m
}

/// Colores (en [0,1]; Godot guarda el color de vértice en 8 bits).
pub const PASTO_BASE: [f32; 4] = [0.50, 0.50, 0.44, 1.0];
pub const PASTO_PUNTA: [f32; 4] = [1.0, 1.0, 0.94, 1.0];
pub const CORTADERA_BASE: [f32; 4] = [0.36, 0.37, 0.22, 1.0];
pub const CORTADERA_PUNTA: [f32; 4] = [0.72, 0.68, 0.43, 1.0];
pub const CORTADERA_TALLO: [f32; 4] = [0.64, 0.60, 0.43, 1.0];
pub const CORTADERA_PENACHO: [f32; 4] = [0.97, 0.94, 0.84, 1.0];

/// Mata de pasto: varias hojas finas inclinadas hacia afuera.
/// UV.y = altura relativa (0 base, 1 punta) para el viento del shader.
pub fn malla_mata_pasto(semilla: u64, hojas: u32, alto: f32, ancho: f32) -> MallaDatos {
    malla_mata(semilla, hojas, alto, ancho, PASTO_BASE, PASTO_PUNTA)
}

pub fn malla_mata(
    semilla: u64,
    hojas: u32,
    alto: f32,
    ancho: f32,
    base_col: [f32; 4],
    punta_col: [f32; 4],
) -> MallaDatos {
    let mut m = MallaDatos::default();
    let mut az = Azar::new(semilla);
    let arriba = [0.0, 1.0, 0.0];
    for h in 0..hojas {
        let ang = h as f32 / hojas as f32 * std::f32::consts::TAU + az.rango(-0.4, 0.4);
        let (s, c) = ang.sin_cos();
        let alto_h = alto * az.rango(0.55, 1.15);
        let incl = az.rango(0.15, 0.55); // inclinación hacia afuera
        let w = ancho * az.rango(0.7, 1.2);
        let base_r = az.rango(0.0, 0.05);
        // dirección perpendicular para el ancho de la hoja
        let (px, pz) = (-s, c);
        let bx = c * base_r;
        let bz = s * base_r;
        let medio = 0.55f32;
        let mx = bx + c * incl * alto_h * medio * 0.6;
        let mz = bz + s * incl * alto_h * medio * 0.6;
        let tx = bx + c * incl * alto_h;
        let tz = bz + s * incl * alto_h;
        let i0 = m.vertices.len() as i32;
        let verts = [
            ([bx - px * w, 0.0, bz - pz * w], 0.0),
            ([bx + px * w, 0.0, bz + pz * w], 0.0),
            ([mx - px * w * 0.7, alto_h * medio, mz - pz * w * 0.7], medio),
            ([mx + px * w * 0.7, alto_h * medio, mz + pz * w * 0.7], medio),
            ([tx, alto_h, tz], 1.0),
        ];
        for (v, t) in verts {
            m.vertices.push(v);
            m.normales.push(arriba);
            let col = [
                base_col[0] + (punta_col[0] - base_col[0]) * t,
                base_col[1] + (punta_col[1] - base_col[1]) * t,
                base_col[2] + (punta_col[2] - base_col[2]) * t,
                1.0,
            ];
            m.colores.push(col);
            m.uvs.push([0.5, t]);
        }
        m.indices
            .extend_from_slice(&[i0, i0 + 2, i0 + 1, i0 + 1, i0 + 2, i0 + 3, i0 + 2, i0 + 4, i0 + 3]);
    }
    m
}

/// Cortadera (paja brava): hojas largas arqueadas y algunos penachos claros.
pub fn malla_cortadera(semilla: u64) -> MallaDatos {
    let mut m = malla_mata(semilla, 22, 1.25, 0.022, CORTADERA_BASE, CORTADERA_PUNTA);
    let mut az = Azar::new(semilla ^ 0xC0FFEE);
    let arriba = [0.0, 1.0, 0.0];
    let penachos = 3 + az.entero(4);
    for _ in 0..penachos {
        let ang = az.rango(0.0, std::f32::consts::TAU);
        let (s, c) = ang.sin_cos();
        let alto = az.rango(1.6, 2.3);
        let incl = az.rango(0.05, 0.22);
        let tx = c * incl * alto;
        let tz = s * incl * alto;
        // tallo fino
        let w = 0.008;
        let (px, pz) = (-s, c);
        let i0 = m.vertices.len() as i32;
        let tallo = [
            [-px * w, 0.0, -pz * w],
            [px * w, 0.0, pz * w],
            [tx * 0.8 - px * w, alto * 0.8, tz * 0.8 - pz * w],
            [tx * 0.8 + px * w, alto * 0.8, tz * 0.8 + pz * w],
        ];
        for (k, v) in tallo.iter().enumerate() {
            m.vertices.push(*v);
            m.normales.push(arriba);
            m.colores.push(CORTADERA_TALLO);
            m.uvs.push([0.5, if k < 2 { 0.0 } else { 0.8 }]);
        }
        m.indices
            .extend_from_slice(&[i0, i0 + 2, i0 + 1, i0 + 1, i0 + 2, i0 + 3]);
        // penacho: dos planos cruzados en forma de huso
        let largo = az.rango(0.35, 0.55);
        let ancho = az.rango(0.05, 0.08);
        for plano in 0..2 {
            let (qx, qz) = if plano == 0 { (px, pz) } else { (c, s) };
            let j0 = m.vertices.len() as i32;
            let y0 = alto * 0.78;
            let pts = [
                ([tx * 0.78, y0, tz * 0.78], 0.8),
                ([tx * 0.9 - qx * ancho, y0 + largo * 0.45, tz * 0.9 - qz * ancho], 0.9),
                ([tx * 0.9 + qx * ancho, y0 + largo * 0.45, tz * 0.9 + qz * ancho], 0.9),
                ([tx, y0 + largo, tz], 1.0),
            ];
            for (v, t) in pts {
                m.vertices.push(v);
                m.normales.push(arriba);
                m.colores.push(CORTADERA_PENACHO);
                m.uvs.push([0.5, t]);
            }
            m.indices
                .extend_from_slice(&[j0, j0 + 1, j0 + 2, j0 + 1, j0 + 3, j0 + 2]);
        }
    }
    m
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn validar(m: &MallaDatos) {
        let n = m.vertices.len();
        assert!(n > 0);
        assert_eq!(m.normales.len(), n);
        assert_eq!(m.colores.len(), n);
        assert_eq!(m.uvs.len(), n);
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| i >= 0 && (i as usize) < n));
        assert!(m.vertices.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn terreno_valido_y_horario_visto_desde_arriba() {
        let mut rel = Relieve::new(1872, 1.4, 90.0, 14.0);
        rel.manchas.push((0.0, 0.0, 3.0));
        rel.senderos.push(((0.0, 0.0), (8.0, -6.0), 1.4));
        let m = malla_terreno(&rel, 1500.0, 0.6, 1.045, 160);
        validar(&m);
        assert!(m.vertices.len() < 25_000, "demasiados vértices: {}", m.vertices.len());
        // Todas las caras deben quedar en sentido horario vistas desde +Y
        // (pantalla: derecha = +X, arriba = -Z).
        for t in m.indices.chunks(3) {
            let p = |i: i32| {
                let v = m.vertices[i as usize];
                (v[0], -v[2])
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let cruz = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            assert!(cruz < 0.0, "triángulo antihorario: {t:?}");
        }
        // el campamento queda prácticamente a altura 0
        assert!(rel.altura(0.0, 0.0).abs() < 0.1);
        assert!(rel.cobertura_pasto(0.0, 0.0) < 0.2);
        assert!(rel.cobertura_pasto(40.0, 40.0) > 0.9);
    }

    #[test]
    fn matas_validas() {
        validar(&malla_mata_pasto(3, 6, 0.45, 0.03));
        validar(&malla_cortadera(9));
    }
}
