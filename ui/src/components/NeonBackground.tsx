import { useEffect, useRef } from "react";
import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";

const BEAM_COUNT = 20;
const ELECTRIC_BLUE = new THREE.Color(0x3b82f6);
const NEON_PURPLE = new THREE.Color(0x8b5cf6);
const DEEP_PURPLE = new THREE.Color(0x6d28d9);
const HOT_CYAN = new THREE.Color(0x22d3ee);

const PALETTE = [ELECTRIC_BLUE, NEON_PURPLE, DEEP_PURPLE, HOT_CYAN] as const;

interface LaserBeam {
  mesh: THREE.Mesh;
  speed: THREE.Vector3;
  life: number;
  maxLife: number;
  pulseOffset: number;
  pulseSpeed: number;
  baseIntensity: number;
  color: THREE.Color;
}

interface Sparkle {
  mesh: THREE.Mesh;
  life: number;
  maxLife: number;
  speed: THREE.Vector3;
}

function pickFromPalette(): THREE.Color {
  const idx = Math.floor(Math.random() * PALETTE.length) % PALETTE.length;
  return PALETTE[idx] ?? ELECTRIC_BLUE;
}

function randomColor(): THREE.Color {
  const a = pickFromPalette();
  const b = pickFromPalette();
  return a.clone().lerp(b, Math.random());
}

function createBeamGeometry(): THREE.TubeGeometry {
  const startX = (Math.random() - 0.5) * 14;
  const startY = (Math.random() - 0.5) * 10;
  const startZ = -2 + Math.random() * -6;

  const midX = startX + (Math.random() - 0.5) * 8;
  const midY = startY + (Math.random() - 0.5) * 6;
  const midZ = startZ + (Math.random() - 0.5) * 3;

  const endX = midX + (Math.random() - 0.5) * 8;
  const endY = midY + (Math.random() - 0.5) * 6;
  const endZ = startZ + (Math.random() - 0.5) * 3;

  const curve = new THREE.CatmullRomCurve3([
    new THREE.Vector3(startX, startY, startZ),
    new THREE.Vector3(midX, midY, midZ),
    new THREE.Vector3(endX, endY, endZ),
  ]);

  const radius = 0.01 + Math.random() * 0.04;
  return new THREE.TubeGeometry(curve, 64, radius, 8, false);
}

function spawnBeam(scene: THREE.Scene): LaserBeam {
  const color = randomColor();
  const intensity = 2 + Math.random() * 4;

  const material = new THREE.MeshBasicMaterial({
    color: color,
    transparent: true,
    opacity: 0,
    toneMapped: false,
  });
  material.color.multiplyScalar(intensity);

  const geometry = createBeamGeometry();
  const mesh = new THREE.Mesh(geometry, material);
  scene.add(mesh);

  const angle = Math.random() * Math.PI * 2;
  const spd = 0.002 + Math.random() * 0.008;

  return {
    mesh,
    speed: new THREE.Vector3(
      Math.cos(angle) * spd,
      Math.sin(angle) * spd,
      0
    ),
    life: 0,
    maxLife: 300 + Math.random() * 500,
    pulseOffset: Math.random() * Math.PI * 2,
    pulseSpeed: 0.5 + Math.random() * 2,
    baseIntensity: intensity,
    color: color.clone(),
  };
}

function spawnSparkle(
  scene: THREE.Scene,
  position: THREE.Vector3
): Sparkle {
  const geo = new THREE.SphereGeometry(0.015 + Math.random() * 0.02, 6, 6);
  const color = randomColor();
  const mat = new THREE.MeshBasicMaterial({
    color: color.multiplyScalar(4),
    transparent: true,
    opacity: 1,
    toneMapped: false,
  });
  const mesh = new THREE.Mesh(geo, mat);
  mesh.position.copy(position);
  scene.add(mesh);

  return {
    mesh,
    life: 0,
    maxLife: 30 + Math.random() * 40,
    speed: new THREE.Vector3(
      (Math.random() - 0.5) * 0.03,
      (Math.random() - 0.5) * 0.03,
      (Math.random() - 0.5) * 0.01
    ),
  };
}

function disposeBeam(scene: THREE.Scene, beam: LaserBeam) {
  scene.remove(beam.mesh);
  beam.mesh.geometry.dispose();
  (beam.mesh.material as THREE.Material).dispose();
}

function disposeSparkle(scene: THREE.Scene, sparkle: Sparkle) {
  scene.remove(sparkle.mesh);
  sparkle.mesh.geometry.dispose();
  (sparkle.mesh.material as THREE.Material).dispose();
}

export function NeonBackground() {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0a0a0a);

    const camera = new THREE.PerspectiveCamera(
      60,
      window.innerWidth / window.innerHeight,
      0.1,
      100
    );
    camera.position.z = 5;

    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setSize(window.innerWidth, window.innerHeight);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.0;
    container.appendChild(renderer.domElement);

    const composer = new EffectComposer(renderer);
    const renderPass = new RenderPass(scene, camera);
    composer.addPass(renderPass);

    const bloomPass = new UnrealBloomPass(
      new THREE.Vector2(window.innerWidth, window.innerHeight),
      1.8,
      0.6,
      0.1
    );
    composer.addPass(bloomPass);

    const beams: LaserBeam[] = [];
    const sparkles: Sparkle[] = [];

    for (let i = 0; i < BEAM_COUNT; i++) {
      const beam = spawnBeam(scene);
      beam.life = Math.random() * beam.maxLife * 0.8;
      beams.push(beam);
    }

    let frameId: number;

    const animate = () => {
      frameId = requestAnimationFrame(animate);

      for (let i = beams.length - 1; i >= 0; i--) {
        const beam = beams[i]!;
        beam.life++;

        beam.mesh.position.x += beam.speed.x;
        beam.mesh.position.y += beam.speed.y;

        const progress = beam.life / beam.maxLife;
        const fadeIn = Math.min(progress * 5, 1);
        const fadeOut = Math.max(1 - (progress - 0.7) / 0.3, 0);
        const envelope = progress < 0.7 ? fadeIn : fadeIn * fadeOut;

        const pulse =
          0.6 +
          0.4 *
            Math.sin(beam.life * 0.02 * beam.pulseSpeed + beam.pulseOffset);

        const mat = beam.mesh.material as THREE.MeshBasicMaterial;
        mat.opacity = envelope * pulse;
        mat.color
          .copy(beam.color)
          .multiplyScalar(beam.baseIntensity * pulse);

        if (Math.random() < 0.008 && envelope > 0.3) {
          const tubeGeo = beam.mesh.geometry as THREE.TubeGeometry;
          const params = tubeGeo.parameters;
          if (params.path) {
            const t = Math.random();
            const point = params.path.getPoint(t);
            point.add(beam.mesh.position);
            sparkles.push(spawnSparkle(scene, point));
          }
        }

        if (beam.life >= beam.maxLife) {
          disposeBeam(scene, beam);
          beams.splice(i, 1);

          if (beams.length < BEAM_COUNT) {
            beams.push(spawnBeam(scene));
          }
        }
      }

      for (let i = sparkles.length - 1; i >= 0; i--) {
        const s = sparkles[i]!;
        s.life++;
        s.mesh.position.add(s.speed);
        const sMat = s.mesh.material as THREE.MeshBasicMaterial;
        sMat.opacity = 1 - s.life / s.maxLife;
        s.mesh.scale.setScalar(1 - s.life / s.maxLife);

        if (s.life >= s.maxLife) {
          disposeSparkle(scene, s);
          sparkles.splice(i, 1);
        }
      }

      while (beams.length < BEAM_COUNT) {
        beams.push(spawnBeam(scene));
      }

      composer.render();
    };

    animate();

    const handleResize = () => {
      camera.aspect = window.innerWidth / window.innerHeight;
      camera.updateProjectionMatrix();
      renderer.setSize(window.innerWidth, window.innerHeight);
      composer.setSize(window.innerWidth, window.innerHeight);
    };

    window.addEventListener("resize", handleResize);

    return () => {
      cancelAnimationFrame(frameId);
      window.removeEventListener("resize", handleResize);

      for (const beam of beams) {
        disposeBeam(scene, beam);
      }
      for (const s of sparkles) {
        disposeSparkle(scene, s);
      }

      composer.dispose();
      renderer.dispose();
      if (container.contains(renderer.domElement)) {
        container.removeChild(renderer.domElement);
      }
    };
  }, []);

  return (
    <div
      ref={containerRef}
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        width: "100vw",
        height: "100vh",
        zIndex: 0,
        pointerEvents: "none",
      }}
    />
  );
}
