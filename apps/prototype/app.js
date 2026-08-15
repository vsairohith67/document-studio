const tools = [
  ['⧉','Merge PDF','Combine documents'],['⌁','Split PDF','Create separate files'],['↘','Compress PDF','Reduce file size'],['▧','PDF to images','Export selected pages'],
  ['▤','Images to PDF','Build a PDF from images'],['Aa','OCR PDF','Make scans searchable'],['✎','Edit PDF','Add content and markup'],['⌾','Protect PDF','Passwords and permissions']
];
const grid = document.getElementById('toolGrid');
tools.forEach(([icon,name,desc], i) => {
  const b = document.createElement('button'); b.className='tool'; b.innerHTML=`<span class="icon">${icon}</span><strong>${name}</strong><small>${desc}</small>`;
  if(i===0) b.addEventListener('click', openWorkbench); grid.appendChild(b);
});
function openWorkbench(){document.getElementById('homeView').classList.add('hidden');document.getElementById('workbenchView').classList.remove('hidden');}
document.getElementById('backHome').onclick=()=>{document.getElementById('workbenchView').classList.add('hidden');document.getElementById('homeView').classList.remove('hidden');};
document.getElementById('addFiles').onclick=openWorkbench;
const fileList=document.getElementById('fileList');
[['Admissions 2026.pdf',6],['Student forms.pdf',8],['Identity documents.pdf',4]].forEach(([name,count],fi)=>{const g=document.createElement('div');g.className='file-group';g.innerHTML=`<div><span><strong>${name}</strong><small>${count} pages</small></span><button>⋮</button></div><div class="thumbs">${Array.from({length:Math.min(count,4)},(_,i)=>`<div class="thumb ${fi===0&&i===0?'selected':''}"><b>${i+1}</b></div>`).join('')}</div>`;fileList.appendChild(g);});
let timer;document.getElementById('runMerge').onclick=()=>{let value=0;const tray=document.getElementById('jobtray');tray.classList.remove('done');document.getElementById('jobTitle').textContent='Merging documents';document.getElementById('jobDetail').textContent='Inspecting and assembling pages locally…';clearInterval(timer);timer=setInterval(()=>{value+=8;document.getElementById('progress').value=value;if(value>=100){clearInterval(timer);tray.classList.add('done');document.getElementById('jobTitle').textContent='Merge completed and verified';document.getElementById('jobDetail').textContent='18 pages reopened, order checked, output published.';}},180)};
document.getElementById('cancelJob').onclick=()=>{clearInterval(timer);document.getElementById('jobTitle').textContent='Job cancelled';document.getElementById('jobDetail').textContent='Temporary output was removed; originals were unchanged.';document.getElementById('progress').value=0;};
['dragenter','dragover'].forEach(e=>document.getElementById('dropzone').addEventListener(e,ev=>{ev.preventDefault();ev.currentTarget.classList.add('drag')}));
['dragleave','drop'].forEach(e=>document.getElementById('dropzone').addEventListener(e,ev=>{ev.preventDefault();ev.currentTarget.classList.remove('drag');if(e==='drop')openWorkbench()}));

if (location.hash === "#workbench") openWorkbench();
