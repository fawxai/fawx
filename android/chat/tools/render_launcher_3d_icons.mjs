#!/usr/bin/env node
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(SCRIPT_DIR, '../src/main/res/drawable-nodpi');
const SIZE = 1024;
const THREE_BUNDLE_PATH = path.join(SCRIPT_DIR, 'vendor/three-r128.min.js');
const THREE_BUNDLE = await fs.readFile(THREE_BUNDLE_PATH, 'utf8');

const FLAVORS = {
  tangerine: { primary: '#FF8C00', glow: '#FFE0B2', tint: '#331C00' },
  lemon: { primary: '#FFD600', glow: '#FFF9C4', tint: '#332B00' },
  lime: { primary: '#7CB342', glow: '#DCEDC8', tint: '#1A2E0D' },
  blood_orange: { primary: '#D84315', glow: '#FFCCBC', tint: '#2E0D04' },
  grapefruit: { primary: '#E91E63', glow: '#F8BBD0', tint: '#2E0413' }
};

function hexToRgb(hex) {
  const v = hex.replace('#', '');
  return {
    r: parseInt(v.slice(0, 2), 16),
    g: parseInt(v.slice(2, 4), 16),
    b: parseInt(v.slice(4, 6), 16)
  };
}

function rgbToHex({ r, g, b }) {
  const to = (n) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
  return `#${to(r)}${to(g)}${to(b)}`;
}

function mixHex(a, b, t) {
  const ca = hexToRgb(a);
  const cb = hexToRgb(b);
  return rgbToHex({
    r: ca.r + (cb.r - ca.r) * t,
    g: ca.g + (cb.g - ca.g) * t,
    b: ca.b + (cb.b - ca.b) * t
  });
}

function palette(flavor) {
  return {
    color1: mixHex('#050607', flavor.tint, 0.78),
    color2: mixHex(flavor.primary, flavor.glow, 0.08),
    color3: mixHex(flavor.primary, '#FFFFFF', 0.22),
    ring: mixHex(flavor.primary, flavor.glow, 0.34),
    particle: mixHex(flavor.primary, flavor.glow, 0.58),
    glow: mixHex(flavor.primary, flavor.glow, 0.28)
  };
}

const html = `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <style>
    html, body { margin: 0; width: 100%; height: 100%; background: transparent; overflow: hidden; }
    canvas { display: block; width: 100%; height: 100%; }
  </style>
</head>
<body>
  <canvas id="c"></canvas>
  <script>${THREE_BUNDLE}</script>
  <script>
    const canvas = document.getElementById('c');
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100);
    camera.position.z = 4.65;

    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, premultipliedAlpha: false });
    renderer.setSize(${SIZE}, ${SIZE}, false);
    renderer.setPixelRatio(1);
    renderer.setClearColor(0x000000, 0);

    const vertexShader = \
\`\n      uniform float uTime;\n      uniform float uNoiseAmp;\n      uniform float uNoiseFreq;\n\n      varying float vDisplacement;\n      varying vec3 vNormal;\n      varying vec3 vWorldPos;\n\n      vec3 mod289(vec3 x){ return x - floor(x*(1.0/289.0))*289.0; }\n      vec4 mod289(vec4 x){ return x - floor(x*(1.0/289.0))*289.0; }\n      vec4 permute(vec4 x){ return mod289(((x*34.0)+1.0)*x); }\n      vec4 taylorInvSqrt(vec4 r){ return 1.79284291400159 - 0.85373472095314*r; }\n\n      float snoise(vec3 v){\n        const vec2 C = vec2(1.0/6.0, 1.0/3.0);\n        const vec4 D = vec4(0.0, 0.5, 1.0, 2.0);\n        vec3 i = floor(v + dot(v, C.yyy));\n        vec3 x0 = v - i + dot(i, C.xxx);\n        vec3 g = step(x0.yzx, x0.xyz);\n        vec3 l = 1.0 - g;\n        vec3 i1 = min(g.xyz, l.zxy);\n        vec3 i2 = max(g.xyz, l.zxy);\n        vec3 x1 = x0 - i1 + C.xxx;\n        vec3 x2 = x0 - i2 + C.yyy;\n        vec3 x3 = x0 - D.yyy;\n        i = mod289(i);\n        vec4 p = permute(permute(permute(\n          i.z + vec4(0.0, i1.z, i2.z, 1.0))\n          + i.y + vec4(0.0, i1.y, i2.y, 1.0))\n          + i.x + vec4(0.0, i1.x, i2.x, 1.0));\n        float n_ = 0.142857142857;\n        vec3 ns = n_ * D.wyz - D.xzx;\n        vec4 j = p - 49.0*floor(p*ns.z*ns.z);\n        vec4 x_ = floor(j*ns.z);\n        vec4 y_ = floor(j - 7.0*x_);\n        vec4 x = x_ * ns.x + ns.yyyy;\n        vec4 y = y_ * ns.x + ns.yyyy;\n        vec4 h = 1.0 - abs(x) - abs(y);\n        vec4 b0 = vec4(x.xy, y.xy);\n        vec4 b1 = vec4(x.zw, y.zw);\n        vec4 s0 = floor(b0)*2.0 + 1.0;\n        vec4 s1 = floor(b1)*2.0 + 1.0;\n        vec4 sh = -step(h, vec4(0.0));\n        vec4 a0 = b0.xzyw + s0.xzyw*sh.xxyy;\n        vec4 a1 = b1.xzyw + s1.xzyw*sh.zzww;\n        vec3 p0 = vec3(a0.xy, h.x);\n        vec3 p1 = vec3(a0.zw, h.y);\n        vec3 p2 = vec3(a1.xy, h.z);\n        vec3 p3 = vec3(a1.zw, h.w);\n        vec4 norm = taylorInvSqrt(vec4(dot(p0,p0),dot(p1,p1),dot(p2,p2),dot(p3,p3)));\n        p0 *= norm.x; p1 *= norm.y; p2 *= norm.z; p3 *= norm.w;\n        vec4 m = max(0.6 - vec4(dot(x0,x0),dot(x1,x1),dot(x2,x2),dot(x3,x3)), 0.0);\n        m = m * m;\n        return 42.0 * dot(m*m, vec4(dot(p0,x0),dot(p1,x1),dot(p2,x2),dot(p3,x3)));\n      }\n\n      void main(){\n        float t = uTime * 0.25;\n        float n1 = snoise(position * uNoiseFreq + t);\n        float n2 = snoise(position * uNoiseFreq * 2.0 + t * 1.3) * 0.5;\n        float noise = n1 + n2;\n        float displacement = noise * uNoiseAmp;\n\n        vec3 newPos = position + normal * displacement;\n\n        vDisplacement = noise;\n        vNormal = normalize(normalMatrix * normal);\n        vWorldPos = (modelMatrix * vec4(newPos, 1.0)).xyz;\n\n        gl_Position = projectionMatrix * modelViewMatrix * vec4(newPos, 1.0);\n      }\n    \`;

    const fragmentShader = \
\`\n      uniform float uOpacity;\n      uniform vec3 uColor1;\n      uniform vec3 uColor2;\n      uniform vec3 uColor3;\n\n      varying float vDisplacement;\n      varying vec3 vNormal;\n      varying vec3 vWorldPos;\n\n      void main(){\n        vec3 viewDir = normalize(cameraPosition - vWorldPos);\n        float fresnel = pow(1.0 - max(dot(vNormal, viewDir), 0.0), 3.0);\n\n        float t = vDisplacement * 0.5 + 0.5;\n        vec3 color = mix(uColor1, uColor2, t);\n        color = mix(color, uColor3, fresnel * 0.6);\n        color += fresnel * uColor2 * 0.4;\n\n        float alpha = (0.88 + fresnel * 0.12) * uOpacity;\n        gl_FragColor = vec4(color, alpha);\n      }\n    \`;

    const sphereGeo = new THREE.IcosahedronGeometry(1.24, 5);
    const sphereMat = new THREE.ShaderMaterial({
      vertexShader,
      fragmentShader,
      uniforms: {
        uTime: { value: 0 },
        uOpacity: { value: 1.0 },
        uNoiseAmp: { value: 0.25 },
        uNoiseFreq: { value: 1.4 },
        uColor1: { value: new THREE.Color('#1a0a00') },
        uColor2: { value: new THREE.Color('#f59e0b') },
        uColor3: { value: new THREE.Color('#ff6b2b') }
      },
      transparent: true,
      side: THREE.FrontSide
    });
    const sphere = new THREE.Mesh(sphereGeo, sphereMat);
    scene.add(sphere);

    const wireGeo = new THREE.IcosahedronGeometry(1.26, 5);
    const wireMat = new THREE.MeshBasicMaterial({ color: new THREE.Color('#f59e0b'), wireframe: true, transparent: true, opacity: 0.055 });
    const wireMesh = new THREE.Mesh(wireGeo, wireMat);
    scene.add(wireMesh);

    const glowGeo = new THREE.IcosahedronGeometry(1.66, 3);
    const glowMat = new THREE.ShaderMaterial({
      vertexShader: \
\`\n        varying vec3 vNormal;\n        varying vec3 vWorldPos;\n        void main(){\n          vNormal = normalize(normalMatrix * normal);\n          vWorldPos = (modelMatrix * vec4(position, 1.0)).xyz;\n          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);\n        }\n      \`,
      fragmentShader: \
\`\n        uniform vec3 uGlowColor;\n        uniform float uOpacity;\n        varying vec3 vNormal;\n        varying vec3 vWorldPos;\n        void main(){\n          vec3 viewDir = normalize(cameraPosition - vWorldPos);\n          float fresnel = pow(1.0 - max(dot(vNormal, viewDir), 0.0), 2.5);\n          gl_FragColor = vec4(uGlowColor, fresnel * 0.2 * uOpacity);\n        }\n      \`,
      uniforms: { uGlowColor: { value: new THREE.Color('#f59e0b') }, uOpacity: { value: 1.0 } },
      transparent: true,
      side: THREE.BackSide,
      depthWrite: false,
      blending: THREE.AdditiveBlending
    });
    const glowMesh = new THREE.Mesh(glowGeo, glowMat);
    scene.add(glowMesh);

    const particleCount = 1500;
    const particlePositions = new Float32Array(particleCount * 3);
    const particleSizes = new Float32Array(particleCount);
    for (let i = 0; i < particleCount; i++) {
      const theta = Math.random() * Math.PI * 2;
      const phi = Math.acos(2 * Math.random() - 1);
      const r = 2.0 + Math.random() * 3.5;
      particlePositions[i*3] = r * Math.sin(phi) * Math.cos(theta);
      particlePositions[i*3 + 1] = r * Math.sin(phi) * Math.sin(theta);
      particlePositions[i*3 + 2] = r * Math.cos(phi);
      particleSizes[i] = 0.5 + Math.random() * 2.2;
    }

    const particleGeo = new THREE.BufferGeometry();
    particleGeo.setAttribute('position', new THREE.BufferAttribute(particlePositions, 3));
    particleGeo.setAttribute('size', new THREE.BufferAttribute(particleSizes, 1));
    const particleMat = new THREE.ShaderMaterial({
      vertexShader: \
\`\n        attribute float size;\n        varying float vAlpha;\n        void main(){\n          vec4 mvPos = modelViewMatrix * vec4(position, 1.0);\n          float dist = length(position);\n          vAlpha = smoothstep(5.5, 2.0, dist) * 0.62;\n          gl_PointSize = size * (220.0 / -mvPos.z);\n          gl_Position = projectionMatrix * mvPos;\n        }\n      \`,
      fragmentShader: \
\`\n        uniform vec3 uColor;\n        uniform float uOpacity;\n        varying float vAlpha;\n        void main(){\n          float d = length(gl_PointCoord - 0.5) * 2.0;\n          float circle = 1.0 - smoothstep(0.4, 1.0, d);\n          gl_FragColor = vec4(uColor, circle * vAlpha * uOpacity);\n        }\n      \`,
      uniforms: { uColor: { value: new THREE.Color('#f59e0b') }, uOpacity: { value: 1.0 } },
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending
    });
    const particles = new THREE.Points(particleGeo, particleMat);
    scene.add(particles);

    const ringGeo = new THREE.TorusGeometry(2.03, 0.0032, 8, 128);
    const ringMat = new THREE.MeshBasicMaterial({ color: new THREE.Color('#f59e0b'), transparent: true, opacity: 0.15 });
    const ring1 = new THREE.Mesh(ringGeo, ringMat);
    ring1.rotation.x = Math.PI * 0.35;
    ring1.rotation.y = Math.PI * 0.1;
    scene.add(ring1);
    const ring2 = ring1.clone();
    ring2.rotation.x = Math.PI * 0.6;
    ring2.rotation.y = Math.PI * 0.5;
    scene.add(ring2);
    const ring3 = ring1.clone();
    ring3.rotation.x = Math.PI * 0.15;
    ring3.rotation.y = Math.PI * 0.85;
    ring3.scale.set(1.3, 1.3, 1.3);
    scene.add(ring3);

    window.renderIcon = (palette) => {
      sphereMat.uniforms.uColor1.value.set(palette.color1);
      sphereMat.uniforms.uColor2.value.set(palette.color2);
      sphereMat.uniforms.uColor3.value.set(palette.color3);
      wireMat.color.set(palette.ring);
      ringMat.color.set(palette.ring);
      particleMat.uniforms.uColor.value.set(palette.particle);
      glowMat.uniforms.uGlowColor.value.set(palette.glow);

      const t = 9.6;
      sphereMat.uniforms.uTime.value = t;
      sphere.rotation.y = t * 0.08;
      sphere.rotation.x = Math.sin(t * 0.05) * 0.15;
      sphere.scale.setScalar(1 + Math.sin(t * 0.4) * 0.02);

      wireMesh.rotation.copy(sphere.rotation);
      wireMesh.scale.copy(sphere.scale);

      glowMesh.rotation.y = t * 0.06;
      glowMesh.scale.setScalar(1 + Math.sin(t * 0.3) * 0.05);

      particles.rotation.y = t * 0.03;
      particles.rotation.x = Math.sin(t * 0.02) * 0.1;

      ring1.rotation.z = t * 0.12;
      ring2.rotation.z = -t * 0.08;
      ring3.rotation.z = t * 0.05;

      renderer.render(scene, camera);
    };

  </script>
</body>
</html>`;

const browser = await chromium.launch({
  headless: true,
  args: [
    '--enable-webgl',
    '--enable-unsafe-swiftshader',
    '--ignore-gpu-blocklist',
    '--use-angle=swiftshader',
    '--use-gl=angle',
    '--disable-gpu-sandbox'
  ]
});
const page = await browser.newPage({ viewport: { width: SIZE, height: SIZE }, deviceScaleFactor: 1 });
page.on('console', (msg) => console.log(`[page] ${msg.type()}: ${msg.text()}`));
page.on('pageerror', (err) => console.error(`[pageerror] ${err.message}`));
await page.setContent(html, { waitUntil: 'load' });
await page.waitForFunction(() => typeof window.renderIcon === 'function', {}, { timeout: 15000 });
await fs.mkdir(ROOT, { recursive: true });

for (const [name, colors] of Object.entries(FLAVORS)) {
  const p = palette(colors);
  await page.evaluate((pal) => {
    window.renderIcon(pal);
  }, p);
  await page.waitForTimeout(60);
  const outPath = path.join(ROOT, `ic_launcher_fg3d_${name}.png`);
  await page.screenshot({
    path: outPath,
    omitBackground: true
  });
  console.log(`wrote ${outPath}`);
}

await browser.close();
console.log('3D icon render complete.');
