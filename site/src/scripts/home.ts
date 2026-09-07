// GSAP is loaded only on home pages and only when the desktop animation is useful.
const media = window.matchMedia(
  '(min-width: 781px) and (prefers-reduced-motion: no-preference)',
);
let cleanup: (() => void) | undefined;
let loading = false;
async function setup() {
  const area = document.querySelector<HTMLElement>('[data-workflow]');
  if (!area || !media.matches || cleanup || loading) return;
  loading = true;
  try {
    const [{ gsap }, { ScrollTrigger }] = await Promise.all([
      import('gsap'),
      import('gsap/ScrollTrigger'),
    ]);
    if (!media.matches) return;
    gsap.registerPlugin(ScrollTrigger);
    document.documentElement.classList.add('motion-enabled');
    const context = gsap.context(() => {
      const panels = gsap.utils.toArray<HTMLElement>('[data-panel]', area);
      const dots = gsap.utils.toArray<HTMLElement>('.workflow-dots span', area);
      const steps = gsap.utils.toArray<HTMLElement>('[data-step]', area);
      function activate(index: number) {
        panels.forEach((panel, i) =>
          gsap.to(panel, {
            autoAlpha: i === index ? 1 : 0,
            y: i === index ? 0 : i < index ? -20 : 35,
            rotation: i === index ? 0 : i < index ? -1 : 2,
            scale: i === index ? 1 : 0.97,
            duration: 0.55,
            ease: 'power2.out',
            overwrite: true,
          }),
        );
        dots.forEach((dot, i) =>
          gsap.to(dot, {
            backgroundColor: i === index ? '#a96412' : '#c3c7b9',
            scale: i === index ? 1.4 : 1,
            duration: 0.3,
          }),
        );
      }
      activate(0);
      steps.forEach((step, index) =>
        ScrollTrigger.create({
          trigger: step,
          start: 'top 58%',
          end: 'bottom 58%',
          onEnter: () => activate(index),
          onEnterBack: () => activate(index),
        }),
      );
      gsap.to('.floating-terminal', {
        y: -18,
        rotation: 0,
        ease: 'none',
        scrollTrigger: {
          trigger: '.showcase',
          start: 'top 80%',
          end: 'bottom top',
          scrub: 1,
        },
      });
      ScrollTrigger.refresh();
    });
    cleanup = () => {
      context.revert();
      document.documentElement.classList.remove('motion-enabled');
      cleanup = undefined;
    };
  } finally {
    loading = false;
  }
}
media.addEventListener('change', () => {
  if (media.matches) void setup();
  else cleanup?.();
});
void setup();
