type Consent = 'granted' | 'denied' | 'withdrawn';
type AnalyticsWindow = Window & {
  dataLayer?: unknown[];
  gtag?: (...args: unknown[]) => void;
  [key: `ga-disable-${string}`]: boolean;
};
const key = 'img.analytics-consent.v1';
export function initAnalytics(id: string) {
  if (!/^G-[A-Z0-9]+$/.test(id)) return;
  const win = window as unknown as AnalyticsWindow;
  const panel = document.querySelector<HTMLElement>('.consent');
  if (!panel) return;
  let consent: Consent | undefined;
  try {
    const saved = localStorage.getItem(key);
    if (saved === 'granted' || saved === 'denied' || saved === 'withdrawn')
      consent = saved;
  } catch {
    /* Consent remains unset when storage is unavailable. */
  }
  let loaded = false,
    pageSent = false;
  function start() {
    win[`ga-disable-${id}`] = false;
    if (!loaded) {
      win.dataLayer = win.dataLayer || [];
      win.gtag = function () {
        win.dataLayer!.push(arguments);
      };
      win.gtag('consent', 'default', {
        analytics_storage: 'granted',
        ad_storage: 'denied',
        ad_user_data: 'denied',
        ad_personalization: 'denied',
      });
      win.gtag('js', new Date());
      win.gtag('config', id, {
        send_page_view: false,
        allow_google_signals: false,
        allow_ad_personalization_signals: false,
        cookie_path: '/img/',
        cookie_domain: 'none',
        cookie_expires: 60 * 60 * 24 * 30,
        page_location: location.origin + location.pathname,
        page_referrer: '',
      });
      const script = document.createElement('script');
      script.async = true;
      script.src =
        'https://www.googletagmanager.com/gtag/js?id=' + encodeURIComponent(id);
      script.dataset.imgAnalytics = '';
      document.head.append(script);
      loaded = true;
    } else win.gtag?.('consent', 'update', { analytics_storage: 'granted' });
    if (!pageSent) {
      win.gtag?.('event', 'page_view', {
        page_title: document.title,
        page_location: location.origin + location.pathname,
        page_referrer: '',
      });
      pageSent = true;
    }
  }
  function stop() {
    win[`ga-disable-${id}`] = true;
    // No consent-mode pings: disable first, then discard this page's pending queue.
    if (win.dataLayer) win.dataLayer.length = 0;
    document.querySelector('script[data-img-analytics]')?.remove();
    loaded = false;
    const names = document.cookie
      .split(';')
      .map((v) => v.trim().split('=')[0])
      .filter((n) => n === '_ga' || n.startsWith('_ga_'));
    const domains = ['', location.hostname, '.' + location.hostname];
    for (const name of names)
      for (const path of ['/', '/img', '/img/'])
        for (const domain of domains)
          document.cookie = `${name}=; Max-Age=0; path=${path};${domain ? ' domain=' + domain + ';' : ''} SameSite=Lax`;
  }
  function select(value: Consent) {
    consent = value;
    try {
      localStorage.setItem(key, value);
    } catch {
      /* The choice still applies to this page. */
    }
    panel!.hidden = true;
    if (value === 'granted') start();
    else stop();
  }
  panel
    .querySelectorAll<HTMLButtonElement>('[data-consent]')
    .forEach((button) =>
      button.addEventListener('click', () =>
        select(button.dataset.consent as Consent),
      ),
    );
  document.querySelectorAll('[data-consent-open]').forEach((button) =>
    button.addEventListener('click', () => {
      panel.hidden = false;
      panel.querySelector<HTMLButtonElement>('button')?.focus();
    }),
  );
  document.addEventListener('click', (event) => {
    if (consent !== 'granted') return;
    const target = (event.target as Element | null)?.closest<HTMLElement>(
      '[data-track]',
    );
    const destination = target?.dataset.track;
    if (destination && ['gui', 'cli', 'github'].includes(destination))
      win.gtag?.('event', 'entry_click', {
        destination,
        page_location: location.origin + location.pathname,
      });
  });
  window.addEventListener('storage', (event) => {
    if (event.key === key) {
      const value = event.newValue;
      if (value === 'granted' || value === 'denied' || value === 'withdrawn') {
        consent = value;
        panel.hidden = true;
        if (value === 'granted') start();
        else stop();
      }
    }
  });
  if (consent === 'granted') start();
  else {
    win[`ga-disable-${id}`] = true;
    panel.hidden = !!consent;
  }
}
