const tools = ['Merge PDF','Split PDF','Compress PDF','PDF to Images','Images to PDF','OCR PDF','Edit PDF','Protect PDF'];

export default function App() {
  return (
    <div className="shell">
      <aside className="rail" aria-label="Primary navigation">
        <div className="brand">DS</div>
        {['Home','Organize','Optimize','Convert','Edit','OCR','Sign','Protect','Automate','History','Settings'].map((item, i) =>
          <button key={item} className={i === 0 ? 'nav active' : 'nav'} aria-label={item}>{item.slice(0,1)}</button>
        )}
      </aside>
      <main>
        <header><div><p className="eyebrow">PRIVATE DOCUMENT WORKSPACE</p><h1>Good morning, Rohith</h1><p>What would you like to do with your documents?</p></div><span className="local">On this device</span></header>
        <label className="search"><span>⌕</span><input aria-label="Search tools" placeholder="Search tools and commands"/><kbd>Ctrl K</kbd></label>
        <section className="drop"><div className="drop-icon">⇩</div><h2>Drop documents here</h2><p>PDF, Word, Excel, PowerPoint and images</p><button>Add files</button><small>Files stay on your device unless you choose an external feature.</small></section>
        <section><div className="section-head"><h2>Quick tools</h2><button className="link">Customize</button></div><div className="grid">{tools.map((tool, i)=><button className="tool" key={tool}><span className="tool-icon">{['⧉','⌁','↘','▧','▤','Aa','✎','⌾'][i]}</span><strong>{tool}</strong><small>{['Combine documents','Create separate files','Reduce file size','Export selected pages','Build a PDF from images','Make scans searchable','Add content and markup','Passwords and permissions'][i]}</small></button>)}</div></section>
      </main>
    </div>
  );
}
