/**
 * Diagrama animado do fluxo do devsplit: o front aponta pro stage, o devsplit
 * intercepta na :443 e divide por path — uns caminhos caem no localhost (verde),
 * o resto faz passthrough pro stage real (cinza). Os "pacotes" percorrem os
 * caminhos via SMIL `animateMotion` (suportado no WebKitGTK do Tauri; sem libs).
 */

const BG = "#15171a";
const BORDER = "#2a2e34";
const TEXT = "#e6e7e9";
const MUTED = "#9aa0a6";
const ACCENT = "#34d399";

const PATH_LOCAL = "M56,140 L262,140 L454,66";
const PATH_STAGE = "M56,140 L262,140 L454,214";

function Packet({ path, color, begin }: { path: string; color: string; begin: number }) {
  return (
    <circle r={4} fill={color} opacity={0}>
      <animateMotion dur="2.7s" begin={`${begin}s`} repeatCount="indefinite" path={path} />
      <animate
        attributeName="opacity"
        dur="2.7s"
        begin={`${begin}s`}
        repeatCount="indefinite"
        values="0;1;1;1;0"
        keyTimes="0;0.1;0.5;0.85;1"
      />
    </circle>
  );
}

export function SystemFlow() {
  return (
    <svg
      viewBox="0 0 520 280"
      className="h-full w-full"
      role="img"
      aria-label="Fluxo do devsplit: front, interceptação na porta 443, e divisão entre local e stage"
    >
      {/* conectores (atrás dos nós) */}
      <g stroke={BORDER} strokeWidth={2} fill="none">
        <path d="M56,140 L262,140" />
        <path d="M262,140 L454,66" stroke={ACCENT} strokeOpacity={0.45} />
        <path d="M262,140 L454,214" />
      </g>

      {/* anel pulsante sutil no devsplit */}
      <rect x={194} y={107} width={136} height={66} rx={12} fill="none" stroke={ACCENT} strokeWidth={1.5}>
        <animate attributeName="opacity" dur="2.6s" repeatCount="indefinite" values="0.05;0.35;0.05" />
      </rect>

      {/* nós */}
      {/* Front */}
      <g>
        <rect x={10} y={118} width={92} height={44} rx={9} fill={BG} stroke={BORDER} />
        <text x={56} y={138} textAnchor="middle" fontSize={12} fontWeight={600} fill={TEXT}>
          Front
        </text>
        <text x={56} y={152} textAnchor="middle" fontSize={9} fill={MUTED}>
          /stage
        </text>
      </g>

      {/* devsplit */}
      <g>
        <rect x={198} y={111} width={128} height={58} rx={11} fill={BG} stroke={ACCENT} strokeOpacity={0.7} />
        <text x={262} y={135} textAnchor="middle" fontSize={13} fontWeight={700} fill={TEXT}>
          devsplit
        </text>
        <text x={262} y={151} textAnchor="middle" fontSize={9} fill={ACCENT}>
          :443 · TLS
        </text>
      </g>

      {/* local */}
      <g>
        <rect x={388} y={43} width={124} height={46} rx={9} fill={BG} stroke={ACCENT} strokeOpacity={0.6} />
        <text x={450} y={62} textAnchor="middle" fontSize={11} fontWeight={600} fill={TEXT}>
          /transporte
        </text>
        <text x={450} y={77} textAnchor="middle" fontSize={9} fill={ACCENT}>
          localhost:3000
        </text>
      </g>

      {/* stage */}
      <g>
        <rect x={388} y={191} width={124} height={46} rx={9} fill={BG} stroke={BORDER} />
        <text x={450} y={210} textAnchor="middle" fontSize={11} fontWeight={600} fill={TEXT}>
          /∗ resto
        </text>
        <text x={450} y={225} textAnchor="middle" fontSize={9} fill={MUTED}>
          stage real
        </text>
      </g>

      {/* badges de decisão */}
      <text x={372} y={104} textAnchor="end" fontSize={8.5} fontWeight={600} fill={ACCENT}>
        LOCAL
      </text>
      <text x={372} y={182} textAnchor="end" fontSize={8.5} fontWeight={600} fill={MUTED}>
        PASSTHROUGH
      </text>

      {/* pacotes */}
      <Packet path={PATH_LOCAL} color={ACCENT} begin={0} />
      <Packet path={PATH_LOCAL} color={ACCENT} begin={0.9} />
      <Packet path={PATH_LOCAL} color={ACCENT} begin={1.8} />
      <Packet path={PATH_STAGE} color={MUTED} begin={0.45} />
      <Packet path={PATH_STAGE} color={MUTED} begin={1.35} />
      <Packet path={PATH_STAGE} color={MUTED} begin={2.25} />
    </svg>
  );
}
