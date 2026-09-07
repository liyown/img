const locale = document.body.dataset.locale === 'en' ? 'en' : 'zh';
const label = (zh:string,en:string) => locale==='zh'?zh:en;
const dialog = document.querySelector<HTMLDialogElement>('.lightbox');
if (dialog) {
  document.querySelectorAll<HTMLAnchorElement>('[data-lightbox]').forEach(link=>link.addEventListener('click',event=>{
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    const img=dialog.querySelector('img')!;
    img.src=link.href; img.alt=link.dataset.caption || '';
    dialog.querySelector('p')!.textContent=img.alt;
    dialog.showModal();
  }));
  dialog.querySelector('button')!.addEventListener('click',()=>dialog.close());
  dialog.addEventListener('click',event=>{if(event.target===dialog)dialog.close();});
}
document.querySelectorAll<HTMLPreElement>('pre.astro-code').forEach(pre=>{
  const wrapper=document.createElement('div');wrapper.className='code-wrap';
  pre.before(wrapper);wrapper.append(pre);
  const button=document.createElement('button');button.type='button';button.className='code-copy';button.textContent=label('复制','Copy');
  button.setAttribute('aria-label',label('复制代码','Copy code'));
  button.addEventListener('click',async()=>{
    try { await navigator.clipboard.writeText(pre.querySelector('code')?.textContent || pre.textContent || '');button.textContent=label('已复制','Copied'); }
    catch {button.textContent=label('请手动选择复制','Select text to copy');}
    window.setTimeout(()=>{button.textContent=label('复制','Copy');},2000);
  });wrapper.append(button);
});
document.addEventListener('keydown',e=>{if(e.key==='Escape')document.querySelectorAll<HTMLDetailsElement>('.mobile-menu[open]').forEach(x=>x.open=false);});
const gaId=document.body.dataset.gaId;
if(gaId) import('./analytics').then(({initAnalytics})=>initAnalytics(gaId));
