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
│     ├─ narrador.rs           Narrador: guion, voz, subtítulos y puesta en escena del gaucho
│     ├─ personajes.rs         Accesorio (objetos en la mano) y Caminante (recorridos)
│     ├─ ciclo.rs, portal.rs   CicloDia (tarde -> noche) y PortalFogon (cambio de escena)
│     ├─ sintesis.rs, ruido.rs, geometria.rs, azar.rs   (Rust puro, con tests)
└─ tools/
   ├─ build_rust.ps1           compila Rust para PC y Quest y copia a godot/bin
   ├─ exportar_quest.ps1       APK + instalar + abrir + logcat
   ├─ captura.ps1              ejecuta la escena en la PC y guarda una captura PNG
   ├─ rig/riggear.py           riggeo y animaciones automáticas de personajes (Blender)
   ├─ voz/                     voz del narrador (Qwen3-TTS, acento argentino)
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
- Cielo de atardecer con nubes y luciérnagas. Sin carteles: los versos los dice el narrador, con
  subtítulos (texto según las primeras ediciones, 1872 y 1879).
- Los animales "respiran" (animación sutil hecha en Rust).

Cada objeto es un nodo `ModeloIA` que carga `godot/modelos/<nombre>.glb`: si regenerás un modelo,
aparece solo en la escena (también en el editor). Las copias múltiples (cardos, vacas, ñandúes,
teros) las siembra `DispersorModelos` sobre el terreno.

## Relato: del atardecer a la noche, y el fortín

La experiencia es **lineal y por tiempo**: nada espera al jugador. Se puede mirar alrededor (y
caminar), pero las escenas avanzan solas y al final vuelve a empezar.

| Momento | Pampa (~2 min 25 s) | Fortín (~1 min 15 s) |
|---|---|---|
| Fundido desde negro + título (el gaucho asa la carne) | 0–11 s | 0–8 s |
| El gaucho se presenta | 12–38 s | — |
| El gaucho recita | desde ~41 s | desde 9 s |
| Anochecer (70 s) | ~71 s → noche cerrada a ~141 s | ya es de noche |
| El fuego se lleva al jugador | ~142 s → fortín | ~71 s → pampa |

1. **Rancho, al atardecer.** Mientras aparece el título, el gaucho está asando la carne. Después
   se da vuelta, saluda tocándose el sombrero ("¡Buenas tardes, paisanos!"), cuenta dónde
   estamos mostrando el campo, señala el asado, camina hasta su lugar y, mate en mano, invita:
   "mientras tomamos unos mates les voy a contar algo". Ahí recita versos del poema (edición de
   1894, ortografía original), con voz y subtítulos sobre su cabeza, mirando al jugador.
2. Con "Y verlos al cair la tarde…" empieza a **anochecer** (`CicloDia`, ~70 s): el sol se pone,
   el cielo pasa por el crepúsculo a una **noche estrellada** con luna, Vía Láctea y las Tres
   Marías; el fogón y el farol cobran fuerza, aparecen las luciérnagas, calma el viento y suben
   los grillos.
3. "…Me echaron á la frontera": el gaucho invita a **mirar el fuego**. Cuando termina de hablar,
   el fuego crece solo y la vista se funde en naranja y negro…
4. **El fortín de frontera** (Canto III): empalizada, mangrullo, cuadras de soldados y el cañón,
   de noche. Hay vida alrededor: un **centinela** con fusil otea el horizonte desde el mangrullo
   (alumbrado por un farol), el **sargento** hace la ronda, un **gaucho levado** cava con la pala
   y un **soldado** toma mate junto al fuego. El gaucho cuenta la leva y el trabajo sin paga; su
   fuego lleva de vuelta al pago.

Atajos de prueba en la PC: **N** salta a la siguiente línea del narrador; `-- --ciclo=1.0` arranca
de noche y `-- --portal` pasa de escena enseguida (por ejemplo con `tools\captura.ps1 -Extra`).

El guion está en `godot/guion/rancho.json` y `fortin.json`. Cada línea tiene texto, audio,
`espera` (pausa antes de la línea), `evento` (`anochecer`, `noche`, `portal`) y la puesta en
escena del gaucho: `accion` (animación mientras habla: `saludar`, `mostrar`, `senalar`, `mate`,
`hablar`…), `mirar` (`jugador` o un nodo, p. ej. `Asador`) e `ir_a` (camina hasta un nodo antes
de hablar, p. ej. `PuntoCharla`). El evento `portal` programa el fundido 1,2 s después de que
termina esa línea. Cada `PortalFogon` tiene además `tiempo_maximo` (200 s y 110 s) como red de
seguridad.

### Voz del narrador (Qwen3-TTS, local)

**Voz masculina con acento argentino (rioplatense).** Qwen3-TTS no trae ninguna voz argentina
(ni el modelo local ni la API en la nube), así que el acento se toma de grabaciones reales: el
modelo **Base** de Qwen3-TTS clona a un hablante del *Crowdsourced high-quality Argentinian
Spanish speech data set* de Google (voluntarios de Buenos Aires, grabado para TTS, CC BY-SA 4.0).
Se eligió el hablante **2484** (de los más graves, ~100 Hz, y el que mejor se entiende).

```powershell
$py = "E:\ComfyUI\ComfyUIStudio\App\python_embeded\python.exe"
# 1) Probar candidatos: deja muestras en tools\voz\trabajo\muestra_ar_<hablante>.wav
& $py -I -B tools\voz\referencia_argentina.py --probar
& $py -I -B tools\voz\referencia_argentina.py --hablantes 8784,1523 --probar
# 2) Elegir uno y regenerar todas las líneas
& $py -I -B tools\voz\referencia_argentina.py --usar 2484
& $py -I -B tools\voz\generar_voz.py --rehacer
```

`generar_voz.py` clona la voz de referencia (`tools/voz/voz_gaucho_referencia.wav`) para cada
línea de los guiones, la verifica con Whisper (si no se entiende, la regenera) y la exporta a
`godot/voz/*.ogg`. La voz anterior, diseñada con VoiceDesign (sin acento argentino), quedó en
`voz_gaucho_disenada.wav`; `generar_voz.py --nueva-voz` diseña otra desde una descripción.

Alternativa probada: **Tomás** (`es-AR-TomasNeural`), voz neural masculina argentina de Microsoft
(servicio online; para uso comercial, Azure Speech de pago):

```powershell
& $py -I -m pip install --target tools\voz\.deps edge-tts    # una sola vez, queda en el proyecto
& $py -I -B tools\voz\voz_microsoft.py --muestras             # normal / pausado / gaucho
& $py -I -B tools\voz\voz_microsoft.py --estilo pausado       # reemplaza todas las líneas
```

### Personajes animados (riggeo automático con Blender)

Los modelos de la IA son estáticos. Las **personas** se generan en pose A (`*_rig` en
`assets.json`) y `tools/rig/riggear.py` las riggea solas con **Blender 4.5 LTS** (portable, en
`%LOCALAPPDATA%\Programs\Blender`, sin instalar nada):

1. estima las articulaciones a partir de la malla (hombros, codos, muñecas, cadera, rodillas…)
   y la orientación de las palmas;
2. arma el esqueleto (con dedos y mandíbula) y lo pesa con los pesos automáticos de Blender;
3. crea por código (cinemática directa + IK de brazos) las animaciones `idle`, `hablar`,
   `saludar`, `mostrar`, `senalar`, `mate`, `caminar`, `asar`, `mirar` y `cavar`: en cada pose
   se orienta la palma (hacia el muslo en reposo, arriba al explicar) y se cierran los dedos
   sobre lo que sostiene;
4. exporta `godot/modelos/animados/<nombre>.glb`, que reemplaza al estático solo.

**La boca del narrador se mueve con su voz**: `tools/voz/envolventes.py` calcula el volumen de
cada línea (`godot/voz/<línea>.boca.json`, 30 valores por segundo; `generar_voz.py` lo hace solo)
y el `Narrador` abre la mandíbula con el modificador `Mandibula` (después de la animación).

```powershell
$B = "$env:LOCALAPPDATA\Programs\Blender\blender-4.5.16-windows-x64\blender.exe"
& $B -b --factory-startup -P tools\rig\riggear.py -- godot\modelos\soldado_rig.glb godot\modelos\animados\soldado.glb --previas build\previas_soldado
```

En Godot: `Accesorio` cuelga un objeto de un hueso, en el punto de agarre dentro de la mano
cerrada (el mate y el fusil derechos; la pala de una mano a la otra, con la hoja hacia el suelo) y `Caminante` lleva a un personaje por puntos (la ronda del
sargento). Los **animales** todavía se animan a mano con [Mesh2Motion](https://mesh2motion.org):
la tabla está en `godot/modelos/animados/LEEME.txt`.

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
- Voz del narrador: Qwen3-TTS (Alibaba/Qwen, Apache-2.0), clonando al hablante 2484 del
  *Crowdsourced high-quality Argentinian Spanish speech data set* (Google, OpenSLR SLR61,
  CC BY-SA 4.0; Guevara-Rukoz et al., LREC 2020). Las líneas generadas (`godot/voz/`) se
  comparten bajo la misma licencia CC BY-SA 4.0.
- Godot Engine (MIT), godot-rust/gdext (MPL-2.0), Godot OpenXR Vendors (MIT).
- Z-Image-Turbo (Tongyi-MAI, Alibaba) y Hunyuan3D-2 (Tencent, *Tencent Hunyuan Community
  License*: revisá sus condiciones antes de un uso comercial; excluye la UE, el Reino Unido y
  Corea del Sur).
