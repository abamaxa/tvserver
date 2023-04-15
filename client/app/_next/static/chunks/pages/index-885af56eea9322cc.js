(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[405],{8312:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/",function(){return s(1653)}])},6877:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return c}});var r=s(5893),a=s(7294),i=s(9518),n=s(5697);class l{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.addEventListener("open",t=>{console.log("open websocket event"),void 0!==e.socket&&e.socket.send("Hello Server!"),e.open=!0}),this.socket.addEventListener("message",function(t){e.onReceive(t).catch(e=>(0,i.hV)("onReceive exception: ".concat(e)))})}},this.close=(e,t)=>{void 0!==this.socket&&(this.socket.close(e,t),this.socket=void 0,this.open=!1,this.listening=!1)},this.send=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.onReceive=async e=>{void 0!==this.socket&&this.open&&void 0!==this.onMessage&&(e.data instanceof n.string?(0,i.hV)(e.data):await this._parseMessage(e))},this._parseMessage=async e=>{try{let t=await new Response(e.data).text();(0,i.hV)("onReceive: ".concat(t));let s=JSON.parse(t);this.onMessage(s)}catch(t){(0,i.tu)("error ".concat(t,", received unexpected message: ").concat(e.data))}},this.isReady=()=>void 0!==this.socket&&this.open,l._instance)return l._instance;l._instance=this,l.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}let o=e=>{let[t,s]=(0,a.useState)(""),n=(0,a.useRef)(null);(0,a.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host;new l(()=>new WebSocket("ws://".concat(t,"/remote/ws")),e=>{if(void 0!==e.Play)s(e.Play.url);else if(void 0!==e.Seek&&null!==n){let t=n.current;t.currentTime=t.currentTime+e.Seek.interval}else if(void 0!==e.TogglePause&&null!==n){let e=n.current;o(e)}})},[e.host]);let o=e=>{e.paused?e.play().catch(e=>{(0,i.FO)(e.message)}):e.pause()},c=e=>{(0,i.hV)("getNextVideo called")},d=e=>{let t=e.target.error;t&&(0,i.hV)("Video error: ".concat(t.message))},h=e=>{e.preventDefault(),(0,i.hV)("keyPress: ".concat(e.key,", code: ").concat(e.code))};return""!=t?(0,r.jsxs)(r.Fragment,{children:[(0,r.jsx)("div",{className:"absolute top-0 left-0 h-screen w-screen bg-transparent z-10",onClick:e=>o(n.current),onKeyUp:e=>h(e)}),(0,r.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,r.jsx)("video",{className:"m-auto w-full h-screen object-contain overflow-hidden",onEnded:e=>c(e),onError:e=>d(e),id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:t,ref:n})})]}):(0,r.jsx)("h1",{className:"text-6xl text-white bg-black text-center h-screen py-32",children:"Ready"})};var c=o},1653:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return em}});var r,a,i,n,l,o,c,d,h,u,m=s(5893),x=s(6877),g=s(9518),p=s(7294),v=s(765);class y{constructor(e,t){this.remote_address="",this.message=e,void 0!==t&&(this.remote_address=t)}}class f{constructor(){this.collection="",this.parent_collection="",this.child_collections=[],this.videos=[],this.errors=[]}}(r=o||(o={})).YouTube="youtube",r.PirateBay="piratebay",(a=c||(c={})).Transmission="transmission",a.AsyncProcess="asyncprocess";class b{constructor(e){this.name=e}}class j{constructor(e){this.newName=e}}let k="text-sm font-medium text-gray-900 border border-gray-200 rounded-lg dark:bg-gray-700 dark:border-gray-600 dark:text-white",w="w-full px-4 py-2 border-gray-200 dark:border-gray-600 overflow-hidden",N="piratebay",C="youtube";(i=d||(d={}))[i.OK=200]="OK",i[i.ACCEPTED=202]="ACCEPTED",i[i.NO_CONTENT=204]="NO_CONTENT",i[i.PARTIAL_CONTENT=206]="PARTIAL_CONTENT",i[i.BAD_REQUEST=400]="BAD_REQUEST",i[i.UNAUTHORIZED=401]="UNAUTHORIZED",i[i.FORBIDDEN=403]="FORBIDDEN",i[i.NOT_FOUND=404]="NOT_FOUND",i[i.CONFLICT=409]="CONFLICT",i[i.UNPROCESSABLE_ENTITY=422]="UNPROCESSABLE_ENTITY",i[i.INTERNAL_SERVER_ERROR=500]="INTERNAL_SERVER_ERROR",i[i.NOT_IMPLEMENTED=501]="NOT_IMPLEMENTED";let T=e=>{let t="popup-menu-overlay",s=(0,p.useRef)(null),[r,a]=(0,p.useState)({position:"absolute",width:"".concat(e.target.clientWidth+4,"px"),top:"".concat(e.target.offsetTop+e.target.clientHeight-(void 0!==e.scrollTop?e.scrollTop:0)-4,"px"),left:"".concat(e.target.offsetLeft-2,"px")}),i=(0,m.jsx)("div",{ref:s,children:e.children});(0,p.useEffect)(()=>{let t=s.current,r=e.target.offsetTop-(void 0!==e.scrollTop?e.scrollTop:0);t.clientHeight+r+e.target.clientHeight>window.innerHeight&&a({position:"absolute",width:"".concat(e.target.clientWidth+4,"px"),top:"".concat(r-t.clientHeight+4,"px"),left:"".concat(e.target.offsetLeft-2,"px")})},[e.scrollTop,e.target.clientHeight,e.target.clientWidth,e.target.offsetLeft,e.target.offsetTop]);let n=s=>{var r;(null===(r=s.target)||void 0===r?void 0:r.id)===t&&e.closeMenu()};return(0,m.jsx)("div",{id:t,style:{position:"absolute",top:0,left:0},className:"h-screen w-screen bg-gray-900 bg-opacity-50 dark:bg-opacity-80",onClick:n,children:(0,m.jsx)("div",{style:r,className:"h-auto w-fit z-20 shadow transition-opacity duration-100 rounded divide-y divide-gray-100 border border-gray-600 bg-white text-gray-900 dark:border-none dark:bg-gray-700 dark:text-white",children:i})})};var E=s(9116);let _=e=>{let t="modal-overlay",s=(0,p.useRef)(null),[r,a]=(0,p.useState)({position:"absolute",top:0,left:0,visibility:"hidden"}),i=(0,m.jsx)("div",{ref:s,children:e.children});(0,p.useEffect)(()=>{let e=s.current,t=window.innerHeight,r=window.innerWidth;a({position:"absolute",top:(t-e.offsetHeight)/2,left:(r-e.offsetWidth)/2,visibility:"visible"})},[s]);let n=s=>{var r;(null===(r=s.target)||void 0===r?void 0:r.id)===t&&e.closeMenu()};return(0,m.jsx)("div",{id:t,style:{position:"absolute",top:0,left:0},className:"h-screen w-screen bg-gray-900 bg-opacity-50 dark:bg-opacity-80",onClick:n,children:(0,m.jsx)("div",{style:r,className:"h-auto w-fit z-20 shadow transition-opacity duration-100 rounded divide-y divide-gray-100 border border-gray-600 bg-white text-gray-900 dark:border-none dark:bg-gray-700 dark:text-white",children:i})})},S=e=>{let[t,s]=(0,p.useState)(""),r=async s=>{if(s.preventDefault(),""===t){(0,E.av)("Must enter a new name");return}e.onClose(),await e.player.renameVideo(e.video,t)};return(0,m.jsx)(_,{closeMenu:e.onClose,children:(0,m.jsxs)("div",{className:"p-4",children:[(0,m.jsxs)("div",{className:"mb-2",children:[(0,m.jsx)("div",{className:"mb-2 block",children:(0,m.jsx)(v.__,{htmlFor:"newName",value:"New Name"})}),(0,m.jsx)(v.oi,{id:"newName",placeholder:e.video,value:t,onChange:e=>s(e.target.value),required:!0})]}),(0,m.jsxs)("div",{className:"flex flex-wrap items-center gap-4",children:[(0,m.jsx)(v.zx,{onClick:e=>r(e),children:"Ok"}),(0,m.jsx)(v.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})},R=e=>{let[t,s]=(0,p.useState)(""),[r,a]=(0,p.useState)([]);(0,p.useEffect)(()=>{let t=async()=>{let t=await e.player.getAvailableConversions();return t.length>0&&s(t[0].name),a(t)};t().catch(e=>(0,g.tu)(e))},[e.player]);let i=async s=>{if(""===t){(0,E.av)("Select a conversion method");return}e.onClose(),await e.player.convertVideo(e.video,t)},n=e=>{let t=e.currentTarget;s(t.value)},l=r.map((e,t)=>(0,m.jsxs)("div",{className:"flex items-center gap-2",children:[(0,m.jsx)(v.Y8,{id:e.name,name:"conversion",value:e.name,defaultChecked:0===t,onChange:n}),(0,m.jsx)(v.__,{htmlFor:e.name,children:e.name})]},t));return(0,m.jsx)(_,{closeMenu:e.onClose,children:(0,m.jsxs)("div",{className:"p-4",children:[(0,m.jsxs)("div",{className:"mb-2",children:[(0,m.jsx)("div",{className:"mb-2 block",children:(0,m.jsx)(v.__,{htmlFor:"conversion",value:"Convert Video"})}),(0,m.jsx)("div",{className:"flex flex-col gap-4 min-w-[16em]",children:l})]}),(0,m.jsxs)("div",{className:"flex pt-4 flex-wrap items-center gap-4",children:[(0,m.jsx)(v.zx,{onClick:e=>i(e),children:"Ok"}),(0,m.jsx)(v.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})},A=(0,m.jsx)(m.Fragment,{}),O=e=>{let t=P(e.isLast)+" text-gray-600",s=e.name,r=t=>{var s,r,a,i,n;t.preventDefault();let l=D(!1),o=null===(s=t.currentTarget)||void 0===s?void 0:null===(r=s.parentNode)||void 0===r?void 0:null===(a=r.parentElement)||void 0===a?void 0:null===(i=a.parentNode)||void 0===i?void 0:null===(n=i.parentElement)||void 0===n?void 0:n.scrollTop,c=(0,m.jsx)(T,{target:t.currentTarget,closeMenu:()=>e.setDialog(A),scrollTop:o,children:(0,m.jsxs)("ul",{children:[(0,m.jsx)("li",{onClick:()=>V(e),className:l+" rounded-t",children:"Play"}),(0,m.jsx)("li",{onClick:t=>I(t,e),className:l,children:"Rename"}),(0,m.jsx)("li",{onClick:t=>L(t,e),className:l,children:"Convert..."}),(0,m.jsx)("li",{onClick:()=>F(e),className:D(!0,"rounded-b"),children:"Delete"})]})});e.setDialog(c)};return(0,m.jsx)("li",{className:t,onClick:e=>r(e),children:(0,m.jsx)("div",{children:s})})},P=e=>{let t=w;return e||(t+=" border-b"),t},D=(e,t)=>{let s=w+" cursor-pointer hover:bg-gray-100 dark:text-gray-200 dark:hover:bg-gray-600 dark:hover:text-white ";return void 0!==t&&(s+=t),e||(s+=" border-b"),s},I=(e,t)=>{let s=()=>t.setDialog(A),r=(0,m.jsx)(S,{onClose:s,video:t.name,player:t.videoPlayer});t.setDialog(r)},L=(e,t)=>{let s=()=>t.setDialog(A),r=(0,m.jsx)(R,{onClose:s,video:t.name,player:t.videoPlayer});t.setDialog(r)},V=e=>{e.setDialog(A),e.videoPlayer.playVideo(e.name)},F=e=>{e.setDialog(A),e.videoPlayer.deleteVideo(e.name)},M=e=>{let[t,s]=(0,p.useState)((0,m.jsx)(m.Fragment,{})),r=e.entry.child_collections.length-1,a=e.entry.videos.length-1,i=(()=>{let t=U(!1),s=e.entry.parent_collection;return""===e.entry.collection?(0,m.jsx)(m.Fragment,{}):(0,m.jsxs)("li",{className:t,onClick:()=>e.setCurrentCollection(s),children:["<-"," Back"]},"0")})(),n=e.entry.child_collections.map((t,s)=>(0,m.jsx)(H,{isLast:s===r&&a<0,name:t,setCurrentCollection:e.setCurrentCollection},t)),l=e.entry.videos.map((t,r)=>(0,m.jsx)(O,{isLast:r===a,name:t,videoPlayer:e.videoPlayer,setDialog:s},t));return(0,m.jsxs)(m.Fragment,{children:[t,(0,m.jsxs)("ul",{className:k,children:[i,n,l]})]})},H=e=>{let t=U(e.isLast);return(0,m.jsx)("li",{className:t,onClick:()=>e.setCurrentCollection(e.name),children:e.name})},U=e=>{let t=w;return e||(t+=" border-b"),t},z=e=>{let[t,s]=(0,p.useState)(new f);return(0,p.useEffect)(()=>{let t=async()=>{try{let t=await e.videoPlayer.fetchCollection();s(t)}catch(e){(0,g.tu)(e)}};if(e.isActive){t();let e=setInterval(async()=>{await t()},2e3);return()=>clearInterval(e)}},[e.videoPlayer.getCurrentCollection,e.videoPlayer,e.isActive]),(0,m.jsx)("div",{className:"flex min-h-full flex-col items-center justify-center p-0",children:(0,m.jsx)("main",{className:"flex w-full flex-1 flex-col items-left justify-left",children:(0,m.jsx)(M,{entry:t,setCurrentCollection:e.videoPlayer.setCurrentCollection,videoPlayer:e.videoPlayer})})})};(n=h||(h={})).TERM="TERM",n.RESULTS="RESULTS",n.ENGINE="ENGINE",n.LAST_SEARCH="LAST_SEARCH";let B=(e,t)=>{let{type:s,payload:r}=t;switch(s){case h.TERM:return{...e,term:r};case h.ENGINE:return{...e,engine:r};case h.RESULTS:return{...e,results:r};case h.LAST_SEARCH:return{...e,lastSearch:r};default:return e}},W=e=>{let t=t=>{let s=t.currentTarget;e.dispatch({type:h.ENGINE,payload:s.value})},s=t=>{e.dispatch({type:h.TERM,payload:t.currentTarget.value})},r=t=>{e.state.term&&e.doSearch(e.state.term,e.state.engine),t.preventDefault()};return(0,m.jsxs)("form",{className:"items-center",onSubmit:e=>r(e),children:[(0,m.jsxs)("div",{className:"flex w-full",children:[(0,m.jsx)("label",{htmlFor:"simple-search",className:"sr-only",children:"SearchTab"}),(0,m.jsxs)("div",{className:"relative w-full",children:[(0,m.jsx)("div",{className:"absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none",children:(0,m.jsx)("svg",{"aria-hidden":"true",className:"w-5 h-5 text-gray-500 dark:text-gray-400",fill:"currentColor",viewBox:"0 0 20 20",xmlns:"http://www.w3.org/2000/svg",children:(0,m.jsx)("path",{fillRule:"evenodd",d:"M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z",clipRule:"evenodd"})})}),(0,m.jsx)("input",{type:"text",id:"search-term",onChange:s,value:e.state.term,className:"bg-gray-50 border border-gray-300 text-gray-900 text-sm rounded-lg focus:ring-blue-500 focus:border-blue-500 block w-full pl-10 p-2.5  dark:bg-gray-700 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white dark:focus:ring-blue-500 dark:focus:border-blue-500",placeholder:"SearchTab",required:!0})]}),(0,m.jsxs)("button",{type:"submit",className:"p-2.5 ml-2 text-sm font-medium text-white bg-blue-700 rounded-lg border border-blue-700 hover:bg-blue-800 focus:ring-4 focus:outline-none focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800",children:[(0,m.jsx)("svg",{className:"w-5 h-5",fill:"none",stroke:"currentColor",viewBox:"0 0 24 24",xmlns:"http://www.w3.org/2000/svg",children:(0,m.jsx)("path",{strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"2",d:"M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"})}),(0,m.jsx)("span",{className:"sr-only",children:"SearchTab"})]})]}),(0,m.jsxs)("div",{className:"flex gap-4 pt-4 px-1",children:[(0,m.jsxs)("div",{className:"flex items-center gap-2",children:[(0,m.jsx)(v.Y8,{id:"piratebay",name:"engine",value:N,defaultChecked:e.state.engine===N,onChange:t}),(0,m.jsx)(v.__,{htmlFor:"piratebay",children:"PirateBay"})]}),(0,m.jsxs)("div",{className:"flex items-center gap-2",children:[(0,m.jsx)(v.Y8,{id:"youtube",name:"engine",value:C,defaultChecked:e.state.engine===C,onChange:t}),(0,m.jsx)(v.__,{htmlFor:"youtube",children:"YouTube"})]})]})]})},K=k+" mt-4",G=e=>{let t=e.results.map((t,s,r)=>{let a=w;return s!=r.length-1&&(a+=" border-b"),(0,m.jsxs)("li",{className:a,onClick:s=>e.onItemClick(t),children:[(0,m.jsx)("p",{children:t.title}),(0,m.jsx)("p",{className:"text-gray-600 text-xs",children:t.description})]},"search:"+s)});if(0!=e.results.length)return(0,m.jsx)("ul",{className:K,children:t});{let t=e.state.lastSearch?"for ".concat(e.state.lastSearch):"";return(0,m.jsxs)("p",{className:"px-1 py-2",children:["No results ",t]})}};class Q{async query(e,t){try{let s=await this.host.get(this.url+e);null!==s.results?t(s.results):null!==s.error&&(0,E.w8)(s.error)}catch(e){(0,g.tu)(e)}}constructor(e,t){this.host=e,this.url=t}}class Y extends Q{constructor(e){super(e,"search/pirate?q=")}}class q extends Q{constructor(e){super(e,"search/youtube?q=")}}class J{async list(e){try{let t=await this.host.get("tasks");null!==t.results?e(t.results):null!==t.error&&(0,g.FO)(t.error)}catch(e){(0,g.tu)(e)}}async add(e){try{await this.host.post("tasks",{name:e.title,link:e.link,engine:e.engine})}catch(e){(0,g.tu)(e)}}async delete(e){let t=[d.OK,d.ACCEPTED,d.NO_CONTENT];(0,E.jt)('Terminate task "'.concat(e.name,'?"'),async()=>{try{let s=await this.host.delete("tasks/".concat(e.taskType,"/").concat(e.key));-1===t.findIndex(e=>e===s.status)&&(0,g.tu)('cannot terminate task "'.concat(e.name,'": "').concat(s.statusText,'"'))}catch(e){(0,g.tu)(e)}})}constructor(e){this.host=e}}let Z=e=>{let t=e.state.results,s=t=>{e.dispatch({type:h.LAST_SEARCH,payload:e.state.term}),e.dispatch({type:h.RESULTS,payload:t})},r=t=>{(0,E.jt)("Download ".concat(t.title,"?"),async()=>{let s=new J(e.host);await s.add(t)})},a=(t,r)=>{let a;switch(r){case"piratebay":a=new Y(e.host);break;case"youtube":a=new q(e.host);break;default:(0,g.tu)("unknown search engine ".concat(r));return}a.query(t,s)};return(0,m.jsxs)("div",{className:"p-0",children:[(0,m.jsx)(W,{doSearch:a,state:e.state,dispatch:e.dispatch}),(0,m.jsx)(G,{state:e.state,results:t,onItemClick:r})]})},X=e=>{let t=[],s=e.result,r=e.index;if(s.finished)return(0,m.jsx)($,{detail:"Finished"},"done:"+r);if(s.sizeDetails&&t.push((0,m.jsx)($,{label:"Size",detail:s.sizeDetails},"size:"+r)),s.eta){let e=ee(s.eta),a="".concat(e," (").concat((100*s.percentDone).toFixed(2),"%)");t.push((0,m.jsx)($,{label:"Eta",detail:a},"eta:"+r))}return s.rateDetails&&t.push((0,m.jsx)($,{label:"Rate",detail:s.rateDetails},"rate:"+r)),s.processDetails&&t.push((0,m.jsx)($,{detail:s.processDetails},"proc:"+r)),(0,m.jsx)(m.Fragment,{children:t})},$=e=>{let t=void 0===e.label?"":"".concat(e.label,": ");return(0,m.jsxs)("p",{className:"text-gray-600 text-xs",children:[t,e.detail,"\xa0"]})},ee=e=>{if(e<=0)return"unknown";let t=[];for(let s of[{period:"day",divisor:86400},{period:"hour",divisor:3600},{period:"min",divisor:60},{period:"sec",divisor:1}]){let r=Math.floor(e/s.divisor);if(e%=s.divisor,0!==r){let e=r>1?s.period+"s":s.period;t.push("".concat(r," ").concat(e))}}return t.join(" ")},et=e=>{let[t,s]=(0,p.useState)([]);(0,p.useEffect)(()=>{if(e.isActive){let t=new J(e.host);(async()=>{await t.list(s)})();let r=setInterval(async()=>{await t.list(s)},2e3);return()=>clearInterval(r)}},[e.isActive,e.host]);let r=async t=>{let s=new J(e.host);await s.delete(t)};return(0,m.jsx)("div",{className:"p-0",children:(0,m.jsx)(es,{results:t,onItemClick:r})})},es=e=>{let t=e.results.map((t,s,r)=>{let a=w;return s!==r.length-1&&(a+=" border-b"),(0,m.jsxs)("li",{className:a,onClick:s=>e.onItemClick(t),children:[(0,m.jsx)("p",{children:t.displayName}),(0,m.jsx)(X,{result:t,index:s})]},"search:"+s)});return 0==e.results.length?(0,m.jsx)("p",{className:"px-1 py-2",children:"No results"}):(0,m.jsx)("ul",{className:k,children:t})};var er=s(3854),ea=s(9274);let ei=e=>{let t=e.player;return(0,m.jsxs)("div",{className:"flex gap-6 p-2 w-full items-center justify-center border-t bg-gray-50",children:[(0,m.jsx)(en,{onClick:()=>t.seek(-15),iconClass:ea.Dfd}),(0,m.jsx)(en,{onClick:()=>t.togglePause(),iconClass:ea.V1r}),(0,m.jsx)(en,{onClick:()=>t.seek(15),iconClass:ea.TuD})]})},en=e=>{let t=p.createElement(e.iconClass,{color:"gray",className:"h-6 w-6"}),s=()=>{e.onClick()};return(0,m.jsx)(v.zx,{color:"gray",className:"border-gray-700",outline:!0,pill:!0,onClick:()=>s(),children:t})};class el{constructor(e,t,s,r){this.getAvailableConversions=async()=>{let e=[];try{let t=await this.host.get("conversion");t.error&&(0,g.tu)("conversions are unavailable: "+t.error),e=t.results||[]}catch(e){(0,g.tu)(e)}return e},this.deleteVideo=async e=>{let t=[d.OK,d.ACCEPTED,d.NO_CONTENT];(0,E.jt)('Delete video "'.concat(e,'?"'),async()=>{try{let s=this.makePath(e),r=await this.host.delete("media/".concat(s));-1===t.findIndex(e=>e===r.status)&&(0,g.tu)('cannot delete "'.concat(e,'": "').concat(r.statusText,'"'))}catch(e){(0,g.tu)(e)}})},this.renameVideo=async(e,t)=>{(0,E.jt)('rename video "'.concat(e,'" to "').concat(t,'"?'),async()=>{try{let s=this.makePath(e),r=this.makePath(t),a=await this.host.put("media/".concat(s),new j(r));a.status!==d.OK&&(0,g.tu)('cannot rename "'.concat(e,'" to "').concat(t,'": "').concat(a.statusText,'"'))}catch(e){(0,g.tu)(e)}})},this.convertVideo=async(e,t)=>{(0,E.jt)("".concat(t,' with "').concat(e,'"?'),async()=>{try{let s=this.makePath(e),r=await this.host.post("media/".concat(s),new b(t));r.status!==d.OK&&(0,g.tu)('cannot convert "'.concat(e,'": "').concat(r.statusText,'"'))}catch(e){(0,g.tu)(e)}})},this.setCurrentCollection=e=>{this.setCurrentCollectionHook(e)},this.getCurrentCollection=()=>this.currentCollection,this.playVideo=e=>{let t={remote_address:this.remote_address,collection:this.currentCollection,video:e};this.post("remote/play",t)},this.seek=e=>{let t=new y({Seek:{interval:e}});this.post("remote/control",t)},this.togglePause=()=>{let e=new y({TogglePause:"ok"});this.post("remote/control",e)},this.post=(e,t)=>{this.host.post(e,t).then(e=>e.json()).then(e=>{e.errors.length>0&&(0,E.w8)(e.errors[0])}).catch(e=>{(0,g.tu)(e)})},this.fetchCollection=async()=>{let e=this.currentCollection?"media/"+this.currentCollection:"media";return await this.host.get(e)},this.makePath=e=>this.currentCollection?"".concat(this.currentCollection,"/").concat(e):e,this.currentCollection=e,this.host=s,this.remote_address=r,this.setCurrentCollectionHook=t}}let eo={term:"",engine:N,results:[],lastSearch:""},ec=e=>{let[t,s]=(0,p.useState)(""),[r,a]=(0,p.useState)(0),[i,n]=(0,p.useState)(!1),[l,o]=(0,p.useReducer)(B,eo),c=new el(t,s,e.host,""),d=(()=>{switch(r){case 1:return(0,m.jsx)(Z,{host:e.host,state:l,dispatch:o});case 2:return(0,m.jsx)(et,{host:e.host,isActive:!0});default:return(0,m.jsx)(z,{host:e.host,videoPlayer:c,isActive:!0})}})();return E.No.setStateFunction(n),(0,m.jsxs)("div",{className:"flex flex-col h-fill-viewport w-full",children:[(0,m.jsx)(E.bZ,{show:i}),(0,m.jsxs)("div",{className:"flex flex-row flex-wrap items-center p-1 w-full",children:[(0,m.jsx)(ed,{name:"Play",tabNumber:0,activeTab:r,setActiveTab:a,iconClass:er.$In}),(0,m.jsx)(ed,{name:"Find",tabNumber:1,activeTab:r,setActiveTab:a,iconClass:er.G4C}),(0,m.jsx)(ed,{name:"Tasks",tabNumber:2,activeTab:r,setActiveTab:a,iconClass:er.EKd})]}),(0,m.jsx)("div",{className:"mb-auto p-1 overflow-y-auto",children:d}),(0,m.jsx)(ei,{player:c})]})},ed=e=>{let t=e.activeTab===e.tabNumber?"white":"gray",s=e.activeTab===e.tabNumber?"info":"gray",r=p.createElement(e.iconClass,{color:t,className:"mr-2 h-4 w-4"});return(0,m.jsxs)(v.zx,{color:s,size:"sm",outline:!1,className:"grow",onClick:()=>e.setActiveTab(e.tabNumber),children:[r," ",e.name]})},eh=new class{constructor(e){this.getHost=()=>void 0!==this.host?this.host:null,this.get=async e=>{let t=await fetch(this.makeUrl(e));return await t.json()},this.post=async(e,t)=>this.send("POST",e,JSON.stringify(t)),this.put=async(e,t)=>this.send("PUT",e,JSON.stringify(t)),this.send=async(e,t,s)=>fetch(this.makeUrl(t),{method:e,body:s,headers:{"Content-Type":"application/json"}}),this.delete=async e=>fetch(this.makeUrl(e),{method:"DELETE"}),this.makeUrl=e=>void 0!==this.host?"http://".concat(this.host,"/").concat(e):"/".concat(e),this.host=e}};(0,g.hu)(eh),(l=u||(u={}))[l.Unknown=1]="Unknown",l[l.Video=2]="Video",l[l.Remote=3]="Remote";let eu=()=>{let[e,t]=(0,p.useState)(u.Unknown);return((0,p.useEffect)(()=>{let e=window.navigator.userAgent;e.includes("SMART-TV")||e.includes("SmartTV")?t(u.Video):t(u.Remote)},[]),e==u.Video)?(0,m.jsx)(x.default,{host:eh}):e==u.Remote?(0,m.jsx)(ec,{host:eh}):(0,m.jsx)("p",{children:"Loading..."})};var em=eu},9116:function(e,t,s){"use strict";s.d(t,{No:function(){return c},av:function(){return h},bZ:function(){return g},jt:function(){return m},jx:function(){return d},w8:function(){return u}});var r,a,i=s(5893),n=s(765),l=s(3854),o=s(9518);(r=a||(a={}))[r.Error=0]="Error",r[r.Information=1]="Information",r[r.Warning=2]="Warning",r[r.Question=3]="Question";let c=new class{constructor(){this.message="",this.show=!1,this.type=a.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=(0,o.HG)(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{let e=this.onOk;this.onOk=void 0,this.hideAlert(),void 0!==e&&e()}}},d=e=>{x(e,a.Error)},h=e=>{x(e,a.Warning)},u=e=>{x(e,a.Information)},m=(e,t)=>{x(e,a.Question,t)},x=(e,t,s)=>{if(void 0!==c)c.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},g=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",r=null;return c.type===a.Information?t=(0,i.jsx)(l.if7,{className:s+"text-gray-400 dark:text-gray-200"}):c.type===a.Error?t=(0,i.jsx)(l.baL,{className:s+"text-red-400 dark:text-red-200"}):c.type===a.Question?(t=(0,i.jsx)(l.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),r=(0,i.jsx)(n.zx,{outline:!0,onClick:()=>c.hideAlert(),children:"Cancel"})):t=(0,i.jsx)(l.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,i.jsxs)(n.u_,{className:"z-50",show:e.show,size:"md",popup:!0,children:[(0,i.jsx)(n.u_.Header,{}),(0,i.jsx)(n.u_.Body,{children:(0,i.jsxs)("div",{className:"text-center",children:[t,(0,i.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:c.message}),(0,i.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,i.jsx)(n.zx,{onClick:()=>c.okClicked(),children:"Ok"}),r]})]})})]})}},9518:function(e,t,s){"use strict";s.d(t,{FO:function(){return c},HG:function(){return h},hV:function(){return o},hu:function(){return n},tu:function(){return d}});var r=s(9116);class a{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}let i=null,n=e=>{i=new a(e)},l=(e,t,s)=>{let r=s?console.error:console.log,a=h(t);if(r("".concat(e," - ").concat(a)),null!==i){let t=[a];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}i.log_messages(e,t)}},o=e=>{l("info",e)},c=e=>{l("warning",e),(0,r.av)(e)},d=e=>{l("error",e,!0),(0,r.jx)(e)},h=e=>"string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e)}},function(e){e.O(0,[556,827,935,774,888,179],function(){return e(e.s=8312)}),_N_E=e.O()}]);