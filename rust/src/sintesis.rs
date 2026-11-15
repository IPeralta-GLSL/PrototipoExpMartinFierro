//! Síntesis procedural del paisaje sonoro: viento pampeano, grillos, fogón
//! y una guitarra criolla (Karplus-Strong). Todo en Rust puro, sin
//! asignaciones de memoria en el camino de audio.

use crate::azar::Azar;
use std::f32::consts::{PI, TAU};

#[inline]
fn coef_pasabajos(fc: f32, sr: f32) -> f32 {
    1.0 - (-TAU * fc / sr).exp()
}

/// Saturación suave para evitar recortes duros.
#[inline]
pub fn saturar(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

#[derive(Clone, Copy, Default)]
struct PasaBajos {
    y: f32,
}

impl PasaBajos {
    #[inline]
    fn procesar(&mut self, x: f32, coef: f32) -> f32 {
        self.y += coef * (x - self.y);
        self.y
    }
}

/// Filtro de estado variable (Chamberlin) para el "silbido" del viento.
#[derive(Clone, Copy, Default)]
struct Svf {
    bajo: f32,
    banda: f32,
}

impl Svf {
    #[inline]
    fn pasabanda(&mut self, x: f32, f: f32, q_inv: f32) -> f32 {
        self.bajo += f * self.banda;
        let alto = x - self.bajo - q_inv * self.banda;
        self.banda += f * alto;
        self.banda
    }
}

// ---------------------------------------------------------------------------
// Viento
// ---------------------------------------------------------------------------

pub struct Viento {
    sr: f32,
    azar: Azar,
    lp_l: PasaBajos,
    lp_r: PasaBajos,
    lp2_l: PasaBajos,
    lp2_r: PasaBajos,
    svf: Svf,
    intensidad: f32,
    objetivo: f32,
    t_cambio: f32,
    contador: u32,
    coef: f32,
    f_svf: f32,
    pub volumen: f32,
}

impl Viento {
    pub fn new(sr: f32, semilla: u64) -> Self {
        Self {
            sr,
            azar: Azar::new(semilla),
            lp_l: PasaBajos::default(),
            lp_r: PasaBajos::default(),
            lp2_l: PasaBajos::default(),
            lp2_r: PasaBajos::default(),
            svf: Svf::default(),
            intensidad: 0.4,
            objetivo: 0.5,
            t_cambio: 0.0,
            contador: 0,
            coef: 0.05,
            f_svf: 0.1,
            volumen: 0.5,
        }
    }

    /// Intensidad actual de la ráfaga (0..1); sirve para mover el pasto.
    pub fn intensidad(&self) -> f32 {
        self.intensidad
    }

    #[inline]
    pub fn muestra(&mut self) -> (f32, f32) {
        let dt = 1.0 / self.sr;
        self.t_cambio -= dt;
        if self.t_cambio <= 0.0 {
            self.objetivo = self.azar.rango(0.15, 1.0);
            self.t_cambio = self.azar.rango(2.5, 8.0);
        }
        // ráfagas lentas (constante de tiempo ~2 s)
        self.intensidad += (self.objetivo - self.intensidad) * (dt / 2.0);
        let i = self.intensidad;

        // coeficientes recalculados cada 32 muestras
        if self.contador % 32 == 0 {
            self.coef = coef_pasabajos(160.0 + 650.0 * i, self.sr);
            let fc = 420.0 + 900.0 * i;
            self.f_svf = 2.0 * (PI * fc / self.sr).sin();
        }
        self.contador = self.contador.wrapping_add(1);

        let wl = self.azar.bipolar();
        let wr = self.azar.bipolar();
        let l = self.lp2_l.procesar(self.lp_l.procesar(wl, self.coef), self.coef);
        let r = self.lp2_r.procesar(self.lp_r.procesar(wr, self.coef), self.coef);
        let silbido = self.svf.pasabanda((wl + wr) * 0.5, self.f_svf, 0.12);

        let amp = 0.35 + 1.1 * i * i.sqrt();
        let s = 0.06 * i * i;
        let k = self.volumen * 2.2;
        (
            saturar(k * (l * amp + silbido * s)),
            saturar(k * (r * amp + silbido * s)),
        )
    }
}

// ---------------------------------------------------------------------------
// Grillos
// ---------------------------------------------------------------------------

struct Grillo {
    frec: f32,
    fase: f32,
    periodo: f32,
    t: f32,
    pulsos: u32,
    dur_pulso: f32,
    sep_pulso: f32,
    amp: f32,
    gan_l: f32,
    gan_r: f32,
}

pub struct Grillos {
    sr: f32,
    azar: Azar,
    grillos: Vec<Grillo>,
    pub volumen: f32,
}

impl Grillos {
    pub fn new(sr: f32, semilla: u64, cantidad: usize) -> Self {
        let mut azar = Azar::new(semilla);
        let grillos = (0..cantidad)
            .map(|_| {
                let pan = azar.rango(0.1, 0.9);
                Grillo {
                    frec: azar.rango(3900.0, 5200.0),
                    fase: 0.0,
                    periodo: azar.rango(0.55, 1.3),
                    t: azar.rango(0.0, 1.0),
                    pulsos: 2 + azar.entero(3),
                    dur_pulso: azar.rango(0.012, 0.02),
                    sep_pulso: azar.rango(0.028, 0.04),
                    amp: azar.rango(0.4, 1.0),
                    gan_l: (pan * PI * 0.5).cos(),
                    gan_r: (pan * PI * 0.5).sin(),
                }
            })
            .collect();
        Self {
            sr,
            azar,
            grillos,
            volumen: 0.08,
        }
    }

    #[inline]
    pub fn muestra(&mut self) -> (f32, f32) {
        let dt = 1.0 / self.sr;
        let (mut l, mut r) = (0.0, 0.0);
        for g in self.grillos.iter_mut() {
            g.t += dt;
            if g.t >= g.periodo {
                g.t -= g.periodo;
                // pequeña irregularidad natural
                g.periodo = (g.periodo * self.azar.rango(0.93, 1.07)).clamp(0.4, 1.6);
            }
            let k = (g.t / g.sep_pulso) as u32;
            let dentro = g.t - k as f32 * g.sep_pulso;
            let env = if k < g.pulsos && dentro < g.dur_pulso {
                (PI * dentro / g.dur_pulso).sin()
            } else {
                0.0
            };
            g.fase += TAU * g.frec * dt;
            if g.fase > TAU {
                g.fase -= TAU;
            }
            if env > 0.0 {
                let s = env * g.fase.sin() * g.amp;
                l += s * g.gan_l;
                r += s * g.gan_r;
            }
        }
        (l * self.volumen, r * self.volumen)
    }
}

// ---------------------------------------------------------------------------
// Fogón: rumor de llamas + chasquidos + estallidos de la leña
// ---------------------------------------------------------------------------

pub struct FuegoSonido {
    sr: f32,
    azar: Azar,
    rumor1: PasaBajos,
    rumor2: PasaBajos,
    lfo: f32,
    env_chasq: f32,
    caida_chasq: f32,
    hp_x: f32,
    hp_y: f32,
    pop_fase: f32,
    pop_frec: f32,
    pop_env: f32,
    pop_caida: f32,
    coef_rumor: f32,
    pub volumen: f32,
    /// chasquidos por segundo (promedio)
    pub tasa: f32,
}

impl FuegoSonido {
    pub fn new(sr: f32, semilla: u64) -> Self {
        Self {
            sr,
            azar: Azar::new(semilla),
            rumor1: PasaBajos::default(),
            rumor2: PasaBajos::default(),
            lfo: 0.0,
            env_chasq: 0.0,
            caida_chasq: 0.99,
            hp_x: 0.0,
            hp_y: 0.0,
            pop_fase: 0.0,
            pop_frec: 120.0,
            pop_env: 0.0,
            pop_caida: 0.999,
            coef_rumor: coef_pasabajos(260.0, sr),
            volumen: 0.8,
            tasa: 9.0,
        }
    }

    /// Una muestra mono (el reproductor 3D la espacializa).
    #[inline]
    pub fn muestra(&mut self) -> f32 {
        let sr = self.sr;
        // rumor grave que "respira"
        self.lfo += TAU * 0.27 / sr;
        if self.lfo > TAU {
            self.lfo -= TAU;
        }
        let w = self.azar.bipolar();
        let rumor = self
            .rumor2
            .procesar(self.rumor1.procesar(w, self.coef_rumor), self.coef_rumor);
        let respiro = 0.75 + 0.25 * self.lfo.sin();

        // chasquidos (proceso de Poisson)
        if self.azar.f32() < self.tasa / sr {
            let a = self.azar.rango(0.25, 1.0);
            self.env_chasq = (self.env_chasq + a * a).min(1.4);
            let tau = self.azar.rango(0.0008, 0.006);
            self.caida_chasq = (-1.0 / (sr * tau)).exp();
        }
        let ruido = self.azar.bipolar();
        // pasa-altos de un polo (~1 kHz) para que el chasquido sea brillante
        self.hp_y = 0.86 * (self.hp_y + ruido - self.hp_x);
        self.hp_x = ruido;
        let chasquido = self.hp_y * self.env_chasq;
        self.env_chasq *= self.caida_chasq;

        // estallidos graves ocasionales de la leña
        if self.azar.f32() < 0.45 / sr {
            self.pop_env = self.azar.rango(0.3, 0.8);
            self.pop_frec = self.azar.rango(70.0, 220.0);
            let tau = self.azar.rango(0.02, 0.06);
            self.pop_caida = (-1.0 / (sr * tau)).exp();
        }
        self.pop_fase += TAU * self.pop_frec / sr;
        if self.pop_fase > TAU {
            self.pop_fase -= TAU;
        }
        let pop = self.pop_fase.sin() * self.pop_env;
        self.pop_env *= self.pop_caida;

        saturar(self.volumen * (rumor * 2.4 * respiro + chasquido * 0.9 + pop * 0.5))
    }
}

// ---------------------------------------------------------------------------
// Guitarra criolla: Karplus-Strong con rasguido
// ---------------------------------------------------------------------------

const MAX_LINEA: usize = 2048;

struct Cuerda {
    linea: [f32; MAX_LINEA],
    largo: usize,
    idx: usize,
    caida: f32,
    activa: bool,
    previo: f32,
}

impl Cuerda {
    fn new() -> Self {
        Self {
            linea: [0.0; MAX_LINEA],
            largo: 2,
            idx: 0,
            caida: 0.996,
            activa: false,
            previo: 0.0,
        }
    }

    fn pulsar(&mut self, frec: f32, sr: f32, amp: f32, azar: &mut Azar) {
        let n = ((sr / frec).round() as usize).clamp(2, MAX_LINEA);
        self.largo = n;
        self.idx = 0;
        // ruido algo filtrado: ataque más cálido, como cuerda de tripa/nylon
        let mut lp = 0.0;
        for v in self.linea[..n].iter_mut() {
            lp += 0.55 * (azar.bipolar() - lp);
            *v = lp * amp;
        }
        // T60 (caída de 60 dB): ~3,6 s en los graves, ~1,9 s en los agudos.
        // La línea se recorre `frec` veces por segundo, así que la ganancia
        // por vuelta es 10^(-3 / (T60 * frec)).
        let t60 = (3.6 - (frec - 82.0) * 0.006).clamp(1.6, 3.6);
        self.caida = (-6.9078 / (t60 * frec)).exp();
        self.activa = true;
    }

    #[inline]
    fn muestra(&mut self) -> f32 {
        if !self.activa {
            return 0.0;
        }
        let n = self.largo;
        let a = self.linea[self.idx];
        let b = self.linea[(self.idx + 1) % n];
        self.linea[self.idx] = self.caida * 0.5 * (a + b);
        self.idx = (self.idx + 1) % n;
        // suavizado leve de salida (cuerpo de madera)
        self.previo += 0.6 * (a - self.previo);
        self.previo
    }
}

/// Acordes para una milonga: Mi menor y Si séptima (afinación estándar).
pub const ACORDE_MI_MENOR: [f32; 6] = [82.41, 123.47, 164.81, 196.00, 246.94, 329.63];
pub const ACORDE_SI_SEPTIMA: [f32; 6] = [0.0, 123.47, 155.56, 220.00, 246.94, 369.99];
pub const ACORDE_LA_MENOR: [f32; 6] = [0.0, 110.00, 164.81, 220.00, 261.63, 329.63];

pub struct Guitarra {
    sr: f32,
    azar: Azar,
    cuerdas: Vec<Cuerda>,
    /// (muestras restantes, índice de cuerda, frecuencia, amplitud)
    pendientes: [(i64, usize, f32, f32); 6],
    pub volumen: f32,
}

impl Guitarra {
    pub fn new(sr: f32, semilla: u64) -> Self {
        Self {
            sr,
            azar: Azar::new(semilla),
            cuerdas: (0..6).map(|_| Cuerda::new()).collect(),
            pendientes: [(-1, 0, 0.0, 0.0); 6],
            volumen: 0.6,
        }
    }

    /// Rasguido hacia abajo (de la 6ª a la 1ª) o hacia arriba.
    pub fn rasguear(&mut self, acorde: &[f32; 6], hacia_abajo: bool, fuerza: f32) {
        let paso = (self.sr * self.azar.rango(0.012, 0.022)) as i64;
        let mut t = 0i64;
        for k in 0..6 {
            let cuerda = if hacia_abajo { k } else { 5 - k };
            let f = acorde[cuerda];
            if f <= 0.0 {
                self.pendientes[cuerda] = (-1, cuerda, 0.0, 0.0);
                continue;
            }
            let amp = fuerza * self.azar.rango(0.75, 1.0);
            self.pendientes[cuerda] = (t, cuerda, f, amp);
            t += paso;
        }
    }

    #[inline]
    pub fn muestra(&mut self) -> f32 {
        for p in self.pendientes.iter_mut() {
            if p.0 == 0 {
                let (_, c, f, a) = *p;
                self.cuerdas[c].pulsar(f, self.sr, a, &mut self.azar);
                p.0 = -1;
            } else if p.0 > 0 {
                p.0 -= 1;
            }
        }
        let mut s = 0.0;
        for c in self.cuerdas.iter_mut() {
            s += c.muestra();
        }
        saturar(s * self.volumen)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn sin_nan_y_acotado(mut f: impl FnMut() -> (f32, f32), n: usize) -> f32 {
        let mut pico: f32 = 0.0;
        for _ in 0..n {
            let (l, r) = f();
            assert!(l.is_finite() && r.is_finite());
            pico = pico.max(l.abs()).max(r.abs());
        }
        assert!(pico <= 1.0, "pico {pico}");
        pico
    }

    #[test]
    fn viento_suena_y_no_explota() {
        let mut v = Viento::new(48_000.0, 1);
        let pico = sin_nan_y_acotado(|| v.muestra(), 48_000 * 10);
        assert!(pico > 0.01, "el viento está mudo");
    }

    #[test]
    fn grillos_y_fuego() {
        let mut g = Grillos::new(48_000.0, 2, 4);
        sin_nan_y_acotado(|| g.muestra(), 48_000 * 5);
        let mut f = FuegoSonido::new(48_000.0, 3);
        let pico = sin_nan_y_acotado(
            || {
                let s = f.muestra();
                (s, s)
            },
            48_000 * 5,
        );
        assert!(pico > 0.01);
    }

    #[test]
    fn guitarra_rasguea_y_decae() {
        let mut g = Guitarra::new(48_000.0, 4);
        g.rasguear(&ACORDE_MI_MENOR, true, 0.9);
        let mut energia_inicio = 0.0;
        let mut energia_final = 0.0;
        for i in 0..48_000 * 4 {
            let s = g.muestra();
            assert!(s.is_finite() && s.abs() <= 1.0);
            if (4_800..14_400).contains(&i) {
                energia_inicio += s * s;
            }
            if i >= 48_000 * 3 {
                energia_final += s * s;
            }
        }
        assert!(energia_inicio > 1.0, "no sonó: {energia_inicio}");
        assert!(energia_final < energia_inicio * 0.5, "no decae");
    }
}
