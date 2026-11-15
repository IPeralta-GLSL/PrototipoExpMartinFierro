# Martín Fierro VR — prototipo

Experiencia VR breve y autóctona para **Meta Quest 3**: un fogón en la pampa bonaerense hacia 1872,
con versos de *El Gaucho Martín Fierro* (José Hernández). Hecha en **Godot 4.7.2** con la lógica en
**Rust** (godot-rust / gdext 0.5.5). Los modelos 3D se generan **en local con modelos chinos**:
Z-Image-Turbo (Alibaba/Tongyi) para la imagen de referencia y Hunyuan3D-2 (Tencent) para la malla
texturizada.

## Accesos rápidos

| Archivo | Qué hace |
|---|---|
| `abrir_editor.bat` | Abre el proyecto en el editor de Godot |
| `jugar_en_pc.bat` | Prueba en la PC sin casco (WASD, botón derecho del mouse para mirar, Shift corre, **G** rasguea la guitarra) |
| `jugar_con_link.bat` | Prueba en VR desde la PC con Meta Quest Link (sin exportar APK: lo más rápido para iterar) |
| `instalar_en_quest.bat` | Compila Rust, exporta el APK, lo instala por USB en el Quest 3 y lo abre |

## En el casco

- **Stick izquierdo**: caminar hacia donde mirás.
- **Stick derecho**: girar de a 30°.
- **Guitarra**: pasá la mano rápido por las cuerdas (o apretá el gatillo cerca) para rasguear una
  milonga (Mi menor / Si séptima), con vibración en el control.
- Mantener apretado el **botón Meta** recentra la vista.

## Estructura

```
PrototipoJuego/
├─ godot/                      proyecto de Godot
│  ├─ escenas/pampa.tscn       escena principal
│  ├─ shaders/                 terreno, pasto con viento, llamas
│  ├─ modelos/                 GLB generados por Hunyuan3D-2
│  ├─ bin/                     bibliotecas de Rust (PC .dll y Quest .so)
│  ├─ addons/godotopenxrvendors/  plugin OpenXR Vendors 5.2.0 (Meta)
│  ├─ martin_fierro.gdextension
│  └─ export_presets.cfg       preset "Meta Quest 3"
├─ rust/                       extensión GDExtension en Rust
│  └─ src/
│     ├─ terreno.rs            TerrenoPampa: relieve procedural, tierra pisada, senderos
│     ├─ pasto.rs              PastoPampa: pasto y cortaderas con MultiMesh por parcelas
│     ├─ ambiente.rs           AmbientePampa (viento + grillos) y Fogon (luz + crepitar)
│     ├─ guitarra.rs           GuitarraCriolla: rasguido con síntesis Karplus-Strong
│     ├─ jugador.rs            JugadorXR: OpenXR, locomoción, modo escritorio, capturas
│     ├─ sintesis.rs, ruido.rs, geometria.rs, azar.rs   (Rust puro, con tests)
└─ tools/
   ├─ build_rust.ps1           compila Rust para PC y Quest y copia a godot/bin
   ├─ exportar_quest.ps1       APK + instalar + abrir + logcat
   ├─ captura.ps1              ejecuta la escena en la PC y guarda una captura PNG
   └─ assetgen/                generación de modelos con IA local (ver abajo)
```

Todo el sonido (viento, grillos, fuego, guitarra) se sintetiza en Rust en tiempo real: no hay
archivos de audio.

### Qué hay en la escena

- **Fogón** con llamas, brasas, humo y luz titilante; alrededor: asado a la cruz, caldera, mate,
  calaveras de caballo como asiento, tronco con la guitarra y el sombrero, el perro echado, el
  recado y un poncho pampa sobre una tranquera.
- **Rancho** de barro y paja con farol, **enramada** con barril, **cuero estaqueado**, **horno de
  barro**, **palenque** con dos caballos criollos (moro y gateado) y la **carreta**.
- **Fondo**: el **ombú**, un corral de palo a pique con un chimango, vacas criollas, ñandúes,
  teros, cardos y cortaderas.
- Cielo de atardecer con nubes, luciérnagas, y tres carteles con estrofas verificadas contra las
  primeras ediciones (1872 y 1879).
- Los animales "respiran" (animación sutil hecha en Rust).

Cada objeto es un nodo `ModeloIA` que carga `godot/modelos/<nombre>.glb`: si regenerás un modelo,
aparece solo en la escena (también en el editor). Las copias múltiples (cardos, vacas, ñandúes,
teros) las siembra `DispersorModelos` sobre el terreno.

## Teaser en video

```powershell
.\tools\trailer.ps1                          # build\teaser_martin_fierro.mp4 (1080p, 30 fps, ~43 s)
.\tools\trailer.ps1 -Resolucion 2560x1440 -Fps 60
```

Se renderiza cuadro a cuadro con el Movie Maker de Godot (`godot/escenas/trailer.tscn` +
`godot/trailer/director.gd`): 7 planos de cámara, la primera sextina repartida en los planos y
una milonga tocada en vivo por la guitarra Karplus-Strong de Rust. Los planos se editan en la
constante `PLANOS` del director.

## Compilar

```powershell
.\tools\build_rust.ps1            # PC (debug) + Quest (release)
.\tools\build_rust.ps1 -SoloPC    # solo la DLL del editor
.\tools\build_rust.ps1 -Tests     # corre los tests de Rust antes
.\tools\exportar_quest.ps1 -Instalar -Ejecutar
.\tools\exportar_quest.ps1 -Logcat   # log de Godot en el casco
```

Con el editor abierto, la DLL se recarga sola al recompilar (`reloadable = true`).

## Modelos con IA local (modelos chinos)

```powershell
$py = "E:\ComfyUI\ComfyUIStudio\App\python_embeded\python.exe"
& $py -B tools\assetgen\generar_assets.py --todo                 # todos (27 modelos)
& $py -B tools\assetgen\generar_assets.py --todo --lote A        # solo el lote prioritario
& $py -B tools\assetgen\generar_assets.py --todo --solo rancho --rehacer   # rehacer uno
& $py -B tools\assetgen\generar_assets.py --previas              # hoja de contactos de los GLB
```

Tiempos en esta PC (RTX 3080 Ti Laptop): ~20–50 s por imagen y ~1–2 min por modelo
texturizado. Para cambiar un objeto, editá su `prompt` o `semilla` en `assets.json` y usá
`--rehacer`.

- Prompts, semillas y tamaños reales en `tools/assetgen/assets.json`.
- Fase 1: Z-Image-Turbo corre en el ComfyUI de ComfyUIStudio (Python embebido), con los pesos de
  `E:\KRITA\ComfyUI\models`, sin modificar ninguna de las dos instalaciones.
- Fase 2: Hunyuan3D-2 (pesos en `E:\ComfyUI\hy3dgen_models`, ~16 GB) con el código de
  ComfyUI-Hunyuan3DWrapper; el rasterizador CUDA compilado se reemplaza por un *shim* sobre
  nvdiffrast (`tools/assetgen/shims`).
- Fase 3: escala real, base en y=0, texturas a 1–2K para el Quest.

## Créditos y licencias

- Versos: José Hernández (1834–1886), *El Gaucho Martín Fierro* (1872) y *La Vuelta de Martín
  Fierro* (1879). Dominio público; texto según las primeras ediciones.
- Godot Engine (MIT), godot-rust/gdext (MPL-2.0), Godot OpenXR Vendors (MIT).
- Z-Image-Turbo (Tongyi-MAI, Alibaba) y Hunyuan3D-2 (Tencent, *Tencent Hunyuan Community
  License*: revisá sus condiciones antes de un uso comercial; excluye la UE, el Reino Unido y
  Corea del Sur).
