import { useEffect, useRef, useMemo } from "react";
import config from "../config/laser-animation.json";

function mulberry32(a: number) {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface LaserLine {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  opacity: number;
  color: string;
  width: number;
  baseDuration: number;
  initialOffset: number;
}

function buildLasers(): LaserLine[] {
  const { lasers, colors } = config;
  const W = 1200, H = 630;
  const rng = mulberry32(lasers.seed);

  const offsets: number[] = [0];
  for (let n = 1; offsets.length < lasers.count; n++) {
    offsets.push(n * lasers.spacing);
    if (offsets.length < lasers.count) offsets.push(-n * lasers.spacing);
  }
  offsets.sort((a, b) => a - b);
  const maxDist = Math.max(...offsets.map(Math.abs)) || 1;

  const lines: LaserLine[] = [];
  for (let i = 0; i < offsets.length; i++) {
    const offset = offsets[i] ?? 0;
    const durJitter = rng() * 2 - 1;
    rng();
    const initialOffset = rng() * (lasers.dash + lasers.gap);
    const lengthRand = 0.5 + rng() * 0.5;

    const perpX = offset * 0.7071;
    const perpY = offset * 0.7071;
    const lengthScale = lasers.length / 100;
    const halfLen = (H * lengthRand * lengthScale) / 2;
    const cx = W / 2 + perpX;
    const cy = H / 2 + perpY;

    const x1 = cx - halfLen;
    const y1 = cy + halfLen;
    const x2 = cx + halfLen;
    const y2 = cy - halfLen;

    const distFromCenter = Math.abs(offset) / maxDist;
    const opacity = Math.max(0, 1 - distFromCenter * (lasers.fade / 100));
    if (opacity <= 0.02) continue;

    const color = colors[Math.abs(i) % colors.length] ?? "#3b82f6";
    const speedFactor = 1 + distFromCenter * 2;
    const baseDuration = (lasers.speed * 0.5 + durJitter * 0.3) * speedFactor;

    lines.push({ x1, y1, x2, y2, opacity, color, width: lasers.width, baseDuration, initialOffset: Math.round(initialOffset % (lasers.dash + lasers.gap)) });
  }
  return lines;
}

interface Props {
  speedMultiplier?: number;
}

export function LaserBackground({ speedMultiplier = 1 }: Props) {
  const svgRef = useRef<SVGSVGElement>(null);
  // Build lines once, never rebuild on speed change
  const lines = useMemo(() => buildLasers(), []);
  const { lasers } = config;
  const cycle = lasers.dash + lasers.gap;

  useEffect(() => {
    const id = "laser-shoot-keyframe";
    let style = document.getElementById(id) as HTMLStyleElement | null;
    if (!style) {
      style = document.createElement("style");
      style.id = id;
      document.head.appendChild(style);
    }
    style.textContent = `@keyframes laser-shoot { 0% { stroke-dashoffset: 0; } 100% { stroke-dashoffset: -${cycle}; } }`;
  }, [cycle]);

  // Set initial animation once, then use playbackRate for smooth speed changes
  const initializedRef = useRef(false);
  const baseSpeedRef = useRef(1);

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;

    if (!initializedRef.current && lasers.gap > 0) {
      // First run: set animation on all lines via DOM
      const allLines = svg.querySelectorAll<SVGLineElement>("line[data-base-dur]");
      allLines.forEach((line) => {
        const base = parseFloat(line.getAttribute("data-base-dur") || "3");
        line.style.animation = `laser-shoot ${base.toFixed(2)}s linear infinite`;
      });
      initializedRef.current = true;
      baseSpeedRef.current = 1;
    }
  }, [lasers.gap]);

  // Smooth speed change via Web Animations API playbackRate
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg || !initializedRef.current) return;

    const allLines = svg.querySelectorAll<SVGLineElement>("line[data-base-dur]");
    allLines.forEach((line) => {
      const anims = line.getAnimations();
      for (const anim of anims) {
        anim.playbackRate = speedMultiplier;
      }
    });
  }, [speedMultiplier]);

  return (
    <svg
      ref={svgRef}
      style={{ position: "fixed", inset: 0, width: "100%", height: "100%", zIndex: 0 }}
      viewBox="0 0 1200 630"
      preserveAspectRatio="xMidYMid slice"
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect width="1200" height="630" fill="#06060e" />
      <defs>
        <filter id="laser-glow">
          <feGaussianBlur stdDeviation={config.glow.blur} result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      <g>
        {lines.map((l, i) => (
          <line
            key={`l${i}`}
            x1={l.x1} y1={l.y1} x2={l.x2} y2={l.y2}
            stroke={l.color}
            strokeWidth={l.width}
            opacity={l.opacity}
            strokeLinecap="round"
            strokeDasharray={lasers.gap > 0 ? `${lasers.dash} ${lasers.gap}` : undefined}
            strokeDashoffset={l.initialOffset}
            data-base-dur={l.baseDuration.toFixed(2)}
          />
        ))}
      </g>
      {config.glow.opacity > 0 && (
        <g filter="url(#laser-glow)" opacity={config.glow.opacity / 100}>
          {lines.map((l, i) => (
            <line
              key={`g${i}`}
              x1={l.x1} y1={l.y1} x2={l.x2} y2={l.y2}
              stroke={l.color}
              strokeWidth={l.width * 2.5}
              opacity={l.opacity}
              strokeLinecap="round"
              strokeDasharray={lasers.gap > 0 ? `${lasers.dash} ${lasers.gap}` : undefined}
              strokeDashoffset={l.initialOffset}
              data-base-dur={l.baseDuration.toFixed(2)}
            />
          ))}
        </g>
      )}
    </svg>
  );
}
