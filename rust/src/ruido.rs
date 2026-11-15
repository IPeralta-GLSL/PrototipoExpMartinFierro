//! Ruido de gradiente (Perlin 2D) y fBm, deterministas y sin dependencias.
//! Se usa para el relieve suave de la pampa y la variación de color del pasto.

use crate::azar::Azar;

pub struct Perlin2 {
    perm: [u8; 512],
}

#[inline]
fn suavizar(t: f32) -> f32 {
    // 6t^5 - 15t^4 + 10t^3
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn mezclar(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn gradiente(hash: u8, x: f32, y: f32) -> f32 {
    match hash & 7 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

impl Perlin2 {
    pub fn new(semilla: u64) -> Self {
        let mut p: [u8; 256] = core::array::from_fn(|i| i as u8);
        let mut azar = Azar::new(semilla);
        for i in (1..256usize).rev() {
            let j = azar.entero(i as u32 + 1) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for (i, v) in perm.iter_mut().enumerate() {
            *v = p[i & 255];
        }
        Self { perm }
    }

    /// Ruido en aproximadamente [-1, 1].
    pub fn ruido(&self, x: f32, y: f32) -> f32 {
        let x0 = x.floor();
        let y0 = y.floor();
        let xf = x - x0;
        let yf = y - y0;
        let xi = ((x0 as i32) & 255) as usize;
        let yi = ((y0 as i32) & 255) as usize;
        let u = suavizar(xf);
        let v = suavizar(yf);
        let p = &self.perm;
        let aa = p[p[xi] as usize + yi];
        let ab = p[p[xi] as usize + yi + 1];
        let ba = p[p[xi + 1] as usize + yi];
        let bb = p[p[xi + 1] as usize + yi + 1];
        let x1 = mezclar(gradiente(aa, xf, yf), gradiente(ba, xf - 1.0, yf), u);
        let x2 = mezclar(
            gradiente(ab, xf, yf - 1.0),
            gradiente(bb, xf - 1.0, yf - 1.0),
            u,
        );
        mezclar(x1, x2, v)
    }

    /// Movimiento browniano fraccional, normalizado a aproximadamente [-1, 1].
    pub fn fbm(&self, x: f32, y: f32, octavas: u32) -> f32 {
        let mut suma = 0.0;
        let mut amp = 1.0;
        let mut frec = 1.0;
        let mut norma = 0.0;
        for o in 0..octavas {
            // desplazamiento por octava para evitar artefactos alineados al origen
            let d = o as f32 * 17.13;
            suma += amp * self.ruido(x * frec + d, y * frec - d);
            norma += amp;
            amp *= 0.5;
            frec *= 2.03;
        }
        suma / norma
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn acotado_y_continuo() {
        let n = Perlin2::new(7);
        let x0 = -100.0f32;
        let mut prev = n.fbm(x0, x0 * 0.37, 4);
        for i in 1..20_000 {
            let x = i as f32 * 0.01 + x0;
            let v = n.fbm(x, x * 0.37, 4);
            assert!(v.is_finite() && v.abs() <= 1.5, "valor fuera de rango: {v}");
            assert!((v - prev).abs() < 0.2, "salto brusco");
            prev = v;
        }
        // valores negativos de coordenadas
        assert!(n.ruido(-3.3, -7.9).is_finite());
    }
}
