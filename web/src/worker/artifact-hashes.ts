// リリースごとの固定ハッシュ（改ざん検知用）。
// graph.json と snap-index.json のハッシュは manifest.json の artifacts に含まれるため、
// ここでは manifest 外の WASM 本体と JS glue の固定値だけを保持する。
// 出典: dist/wasm の実測値とデプロイ済み Workers の一致確認済み（scout-002 F2）。

export interface ArtifactExpectation {
  sha256: string;
  byteLength: number;
}

export interface ReleaseArtifactHashes {
  wasm: ArtifactExpectation;
  glue: ArtifactExpectation;
}

export const ARTIFACT_HASHES: Record<string, ReleaseArtifactHashes> = {
  "c1-real-v1": {
    wasm: {
      sha256: "0fa2c07cb337370b144f0160b782df39d4790b6558dfc1d1caec3d502d8cf731",
      byteLength: 435829,
    },
    glue: {
      sha256: "43ab026ea5be27749e1ab95a40c657815a34fc0f3dfa0b049cf59252c8a980b3",
      byteLength: 7666,
    },
  },
};
