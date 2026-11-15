//! Generador pseudoaleatorio determinista (SplitMix64), sin dependencias.
//! Misma semilla => mismo mundo en PC y en el Quest.

#[derive(Clone, Debug)]
pub struct Azar {
    estado: u64,
}

impl Azar {
    pub fn new(semilla: u64) -> Self {
        Self {
            estado: semilla ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    #[inline]
    pub fn siguiente_u64(&mut self) -> u64 {
        self.estado = self.estado.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.estado;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniforme en [0, 1).
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.siguiente_u64() >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
    }

    /// Uniforme en [min, max).
    #[inline]
    pub fn rango(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.f32()
    }

    /// Uniforme en [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    /// Entero uniforme en [0, n).
    #[inline]
    pub fn entero(&mut self, n: u32) -> u32 {
        (((self.siguiente_u64() >> 32) * n as u64) >> 32) as u32
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn determinista_y_en_rango() {
        let mut a = Azar::new(42);
        let mut b = Azar::new(42);
        for _ in 0..10_000 {
            let x = a.f32();
            assert_eq!(x, b.f32());
            assert!((0.0..1.0).contains(&x));
            let e = a.entero(7);
            b.entero(7);
            assert!(e < 7);
        }
    }
}
