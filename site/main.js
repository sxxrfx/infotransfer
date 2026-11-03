document.documentElement.classList.add('js');
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;

// Scroll progress, nav border and the recording's zoom-in.
const progress = document.querySelector('.progress span');
const nav = document.querySelector('.nav');
const zoom = document.querySelector('.zoom');
function onScroll() {
  const max = document.documentElement.scrollHeight - innerHeight;
  progress.style.setProperty('--p', max > 0 ? scrollY / max : 0);
  nav.classList.toggle('scrolled', scrollY > 8);
  if (zoom && !reduced) {
    const box = zoom.getBoundingClientRect();
    const t = Math.min(1, Math.max(0, (innerHeight - box.top) / (innerHeight * 0.7)));
    zoom.style.setProperty('--z', 0.88 + 0.12 * t);
    zoom.style.setProperty('--o', 0.5 + 0.5 * t);
  }
}
addEventListener('scroll', onScroll, { passive: true });
onScroll();

// Fade elements in as they enter the viewport; siblings are staggered.
let pending = [...document.querySelectorAll('.reveal')];
function reveal() {
  pending = pending.filter((element) => {
    if (element.closest('[hidden]')) return true;
    if (element.getBoundingClientRect().top > innerHeight - 40) return true;
    const siblings = [...element.parentElement.children].filter((child) => child.classList.contains('reveal'));
    if (!element.style.getPropertyValue('--d')) element.style.setProperty('--d', `${Math.min(siblings.indexOf(element), 8) * 0.06}s`);
    element.classList.add('in');
    return false;
  });
}

// Hero demo: select the paragraph, tap the key, then highlight each word as it is "spoken".
const text = document.getElementById('demo-text');
const key = document.getElementById('demo-key');
const pill = document.getElementById('demo-pill');
const label = document.getElementById('demo-label');
const words = text.textContent.split(' ');
text.innerHTML = words.map((word, i) => `<span class="w">${word}${i < words.length - 1 ? ' ' : ''}</span>`).join('');
const spans = [...text.querySelectorAll('.w')];
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function demo() {
  for (;;) {
    spans.forEach((span) => span.classList.remove('sel', 'now'));
    pill.classList.remove('speaking');
    label.textContent = 'Ready';
    await wait(900);
    for (const span of spans) { span.classList.add('sel'); await wait(45); }
    await wait(500);
    key.classList.add('down');
    await wait(160);
    key.classList.remove('down');
    label.textContent = 'Preparing…';
    await wait(400);
    pill.classList.add('speaking');
    label.textContent = 'Reading';
    for (const span of spans) {
      span.classList.add('now');
      await wait(140 + span.textContent.length * 22);
      span.classList.remove('now', 'sel');
    }
    pill.classList.remove('speaking');
    label.textContent = 'Done';
    await wait(2200);
  }
}
if (reduced) {
  spans.forEach((span) => span.classList.add('sel'));
  pill.classList.add('speaking');
  label.textContent = 'Reading';
} else {
  demo();
}

// Sticky "how it works": the step nearest the middle of the screen drives the stage.
const stage = document.getElementById('stage');
const steps = [...document.querySelectorAll('.step')];
function track() {
  const middle = innerHeight / 2;
  let nearest = null;
  let distance = Infinity;
  for (const step of steps) {
    const box = step.getBoundingClientRect();
    const d = Math.abs(box.top + box.height / 2 - middle);
    if (d < distance) { distance = d; nearest = step; }
  }
  const inView = steps[0].getBoundingClientRect().top < innerHeight && steps.at(-1).getBoundingClientRect().bottom > 0;
  steps.forEach((step) => step.classList.toggle('active', inView && step === nearest));
  if (inView) stage.dataset.active = nearest.dataset.step;
}

// Count numbers up once they are visible.
const counters = [...document.querySelectorAll('[data-count]')];
function count() {
  for (const element of counters) {
    if (element.dataset.done || element.getBoundingClientRect().top > innerHeight * 0.9) continue;
    element.dataset.done = 1;
    const target = Number(element.dataset.count);
    const decimals = Number(element.dataset.decimals || 0);
    const suffix = element.dataset.suffix || '';
    const start = performance.now();
    const duration = reduced ? 0 : 1200;
    const tick = () => {
      const t = duration ? Math.min(1, (performance.now() - start) / duration) : 1;
      element.textContent = (target * (1 - Math.pow(1 - t, 3))).toFixed(decimals) + suffix;
      if (t < 1) setTimeout(tick, 16);
    };
    tick();
  }
}

function update() { reveal(); track(); count(); }
addEventListener('scroll', update, { passive: true });
addEventListener('resize', update);
update();

// Install tabs.
const tabs = [...document.querySelectorAll('[role="tab"]')];
tabs.forEach((tab) => tab.addEventListener('click', () => {
  tabs.forEach((other) => {
    const selected = other === tab;
    other.setAttribute('aria-selected', selected);
    document.getElementById(other.getAttribute('aria-controls')).hidden = !selected;
  });
  reveal();
}));

// Copy buttons.
document.querySelectorAll('.copy').forEach((button) => button.addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(button.parentElement.querySelector('code').textContent);
    button.textContent = 'Copied';
  } catch {
    button.textContent = 'Select and copy';
  }
  setTimeout(() => { button.textContent = 'Copy'; }, 1600);
}));
