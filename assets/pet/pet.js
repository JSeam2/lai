import * as THREE from './vendor/three.module.js';
import { GLTFLoader } from './vendor/GLTFLoader.js';

const bridge = (message) => window.webkit.messageHandlers.pet.postMessage(message);
window.addEventListener('error', (event) => bridge({type: 'fatal', message: event.message || 'Pet renderer failed'}));
const cat = document.querySelector('#cat');
const conversation = document.querySelector('#conversation');
const reply = document.querySelector('#reply');
const input = document.querySelector('#message');
const button = document.querySelector('button');
const messages = [];
let mixer, clips = {}, currentAction, sleeping = false;
let animations = { idle: '1_normal', waiting: '6_curious', reply: '2_happy', error: '3_sad', shutdown: '3_sad' };

const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(32, 245 / 280, .01, 100);
camera.position.set(0, .8, 3.4);
camera.lookAt(0, .55, 0);
const renderer = new THREE.WebGLRenderer({ canvas: cat, alpha: true, antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setSize(245, 280, false);
scene.add(new THREE.HemisphereLight(0xffffff, 0x8c849e, 2));
const light = new THREE.DirectionalLight(0xffffff, 2);
light.position.set(2, 4, 4);
scene.add(light);

function expression(name) {
  if (!mixer || !clips[name]) return;
  const next = mixer.clipAction(clips[name]);
  if (next === currentAction) return;
  next.reset().play();
  if (currentAction) currentAction.crossFadeTo(next, .2, false);
  currentAction = next;
}
Promise.all([
  new GLTFLoader().loadAsync('/pet.glb'),
  fetch('/config.json').then((response) => {
    if (!response.ok) throw new Error('configuration unavailable');
    return response.json();
  }),
]).then(([gltf, config]) => {
  animations = config;
  scene.add(gltf.scene);
  mixer = new THREE.AnimationMixer(gltf.scene);
  for (const clip of gltf.animations) clips[clip.name] = clip;
  const missing = Object.entries(animations).filter(([, name]) => !clips[name]);
  if (missing.length) {
    reply.textContent = `Animation not found: ${missing.map(([event, name]) => `${event}: ${name}`).join(', ')}. Check ~/.lai/config.toml.`;
    conversation.hidden = false;
    bridge({type: 'showChat', open: true});
  }
  expression(sleeping ? animations.shutdown : animations.idle);
  bridge({type: 'ready'});
}).catch((error) => {
  reply.textContent = 'Could not load the cat model.';
  conversation.hidden = false;
  bridge({type: 'showChat', open: true});
  bridge({type: 'fatal', message: `Could not load the cat model: ${error.message}`});
});
const clock = new THREE.Clock();
function render() {
  requestAnimationFrame(render);
  if (mixer) mixer.update(clock.getDelta());
  renderer.render(scene, camera);
}
render();

let pointer;
cat.addEventListener('pointerdown', (event) => {
  if (event.button !== 0 || sleeping) return;
  pointer = { x: event.screenX, y: event.screenY, moved: false };
  cat.setPointerCapture(event.pointerId);
  bridge({type: 'dragStart'});
});
cat.addEventListener('pointermove', (event) => {
  if (!pointer) return;
  const dx = event.screenX - pointer.x, dy = event.screenY - pointer.y;
  if (Math.abs(dx) + Math.abs(dy) > 4) pointer.moved = true;
  if (pointer.moved) bridge({type: 'drag', dx, dy});
});
cat.addEventListener('pointerup', () => {
  if (!pointer) return;
  if (!pointer.moved) {
    conversation.hidden = !conversation.hidden;
    bridge({type: 'showChat', open: !conversation.hidden});
    if (!conversation.hidden) input.focus();
  }
  pointer = null;
});
cat.addEventListener('pointercancel', () => { pointer = null; });
window.addEventListener('contextmenu', (event) => { event.preventDefault(); if (!sleeping) bridge({type: 'menu'}); });
document.querySelector('#chat').addEventListener('submit', (event) => {
  event.preventDefault();
  const text = input.value.trim();
  if (!text || button.disabled || sleeping) return;
  messages.push({ role: 'user', content: text });
  input.value = '';
  button.disabled = true;
  reply.textContent = 'Thinking…';
  expression(animations.waiting);
  bridge({type: 'chat', messages});
});
window.laiResponse = (result) => {
  button.disabled = false;
  if (sleeping) return;
  if (result.error) {
    messages.pop();
    reply.textContent = result.error;
    expression(animations.error);
  } else {
    messages.push({role: 'assistant', content: result.text});
    reply.textContent = result.text;
    expression(animations.reply);
  }
};
window.laiSleep = () => {
  sleeping = true;
  button.disabled = true;
  conversation.hidden = true;
  expression(animations.shutdown);
  // Leave the sad face visible briefly before fading the entire pet window.
  setTimeout(() => document.querySelector('#pet').classList.add('sleeping'), 500);
};
