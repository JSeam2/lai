import * as THREE from './vendor/three.module.js';

export function applyToonMaterials(root) {
  const materials = new Map();
  const outlines = new Map();
  function convert(source) {
    if (materials.has(source)) return materials.get(source);
    const options = {};
    for (const key of [
      'name', 'color', 'map', 'alphaMap', 'alphaTest', 'opacity', 'transparent',
      'side', 'vertexColors', 'depthWrite', 'depthTest', 'blending',
      'emissive', 'emissiveMap', 'emissiveIntensity', 'normalMap', 'normalScale',
      'bumpMap', 'bumpScale', 'aoMap', 'aoMapIntensity',
    ]) {
      if (source[key] !== undefined) options[key] = source[key];
    }
    const material = new THREE.MeshToonMaterial(options);
    materials.set(source, material);
    return material;
  }
  function outlineMaterial(source) {
    if (outlines.has(source)) return outlines.get(source);
    const material = new THREE.MeshBasicMaterial({
      color: 0x211c2b, side: THREE.BackSide, map: source.map,
      alphaMap: source.alphaMap, alphaTest: source.alphaTest,
      opacity: source.opacity, transparent: source.transparent,
      depthWrite: source.depthWrite,
    });
    material.onBeforeCompile = (shader) => {
      shader.vertexShader = shader.vertexShader.replace(
        '#include <begin_vertex>',
        '#include <begin_vertex>\ntransformed += normal * 0.0075;',
      );
    };
    outlines.set(source, material);
    return material;
  }
  const meshes = [];
  root.traverse((object) => {
    if (object.isMesh && object.material) meshes.push(object);
  });
  for (const object of meshes) {
    object.material = Array.isArray(object.material)
      ? object.material.map(convert)
      : convert(object.material);
    const material = Array.isArray(object.material)
      ? object.material.map(outlineMaterial)
      : outlineMaterial(object.material);
    const outline = object.isSkinnedMesh
      ? new THREE.SkinnedMesh(object.geometry, material)
      : new THREE.Mesh(object.geometry, material);
    if (object.isSkinnedMesh) outline.bind(object.skeleton, object.bindMatrix);
    outline.morphTargetInfluences = object.morphTargetInfluences;
    outline.morphTargetDictionary = object.morphTargetDictionary;
    outline.raycast = () => {};
    object.add(outline);
  }
}
