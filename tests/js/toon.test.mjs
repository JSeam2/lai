import assert from 'node:assert/strict';
import * as THREE from '../../assets/pet/vendor/three.module.js';
import { applyToonMaterials } from '../../assets/pet/toon.js';

const texture = new THREE.Texture();
const original = new THREE.MeshStandardMaterial({
  color: 0x884422, map: texture, transparent: true, opacity: 0.5,
  alphaTest: 0.3, side: THREE.DoubleSide, vertexColors: true,
});
original.name = 'fur';
const other = new THREE.MeshStandardMaterial({ color: 0x112233 });
const root = new THREE.Group();
const first = new THREE.Mesh(new THREE.BufferGeometry(), original);
const second = new THREE.Mesh(new THREE.BufferGeometry(), [original, other]);
root.add(first, second);
applyToonMaterials(root);
assert.ok(first.material.isMeshToonMaterial);
assert.equal(first.material, second.material[0]);
assert.ok(second.material[1].isMeshToonMaterial);
assert.equal(first.material.name, 'fur');
assert.ok(first.material.color.equals(original.color));
assert.equal(first.material.map, texture);
assert.equal(first.material.transparent, true);
assert.equal(first.material.opacity, 0.5);
assert.equal(first.material.alphaTest, 0.3);
assert.equal(first.material.side, THREE.DoubleSide);
assert.equal(first.material.vertexColors, true);
assert.notEqual(first.material, original);

const outline = first.children[0];
assert.ok(outline?.isMesh);
assert.equal(outline.geometry, first.geometry);
assert.equal(outline.material.side, THREE.BackSide);
assert.equal(outline.material.map, texture);
assert.equal(outline.material.opacity, 0.5);
assert.equal(second.children[0].material[0], outline.material);
const shader = { vertexShader: '#include <begin_vertex>' };
outline.material.onBeforeCompile(shader);
assert.match(shader.vertexShader, /transformed\s*\+=/);

const skinned = new THREE.SkinnedMesh(new THREE.BufferGeometry(), original);
const skeleton = new THREE.Skeleton([new THREE.Bone()]);
skinned.bind(skeleton);
skinned.morphTargetInfluences = [0.4];
const animatedRoot = new THREE.Group();
animatedRoot.add(skinned);
applyToonMaterials(animatedRoot);
assert.ok(skinned.children[0].isSkinnedMesh);
assert.equal(skinned.children[0].skeleton, skeleton);
assert.equal(skinned.children[0].morphTargetInfluences, skinned.morphTargetInfluences);
