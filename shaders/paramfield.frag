// Spatially-varying feed/kill offsets: different regions of the canvas can
// land in different Gray-Scott pattern families at once — the "marble"
// look. Two independent domain-warped fbm layers, one for feed, one for
// kill. Recomputed once per rendered frame; uEvolveT lets it slowly drift.
in vec2 vUv;
out vec4 fragColor;

uniform float uFeedVarAmount;
uniform float uFeedVarScale;
uniform float uFeedVarWarp;
uniform float uKillVarAmount;
uniform float uKillVarScale;
uniform float uKillVarWarp;
uniform float uEvolveT;
uniform float uAspect; // grid width / height — keeps the noise isotropic on screen

void main() {
  vec2 uv = vUv * vec2(uAspect, 1.0);
  float fn = warpedFbm(uv * uFeedVarScale + uEvolveT, uFeedVarWarp, 4) * 2.0 - 1.0;
  float kn = warpedFbm(uv * uKillVarScale + uEvolveT + 37.0, uKillVarWarp, 4) * 2.0 - 1.0;
  fragColor = vec4(fn * uFeedVarAmount, kn * uKillVarAmount, 0.0, 1.0);
}
