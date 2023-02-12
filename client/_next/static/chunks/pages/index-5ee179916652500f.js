(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[405],{8312:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/",function(){return s(8655)}])},830:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return p}});var o=s(5893),n=s(7294);class i{constructor(e,t){this.remote_address="",this.message=e,void 0!==t&&(this.remote_address=t)}}class r{constructor(){this.collection="",this.parent_collection="",this.child_collections=[],this.videos=[],this.errors=[]}}class c{constructor(e,t,s){this.playVideo=e=>{let t={remote_address:this.remote_address,collection:this.currentCollection,video:e};this.post("remote-play",t)},this.seek=e=>{let t=new i({Seek:{interval:e}});this.post("remote-control",t)},this.togglePause=()=>{let e=new i({TogglePause:"ok"});this.post("remote-control",e)},this.post=(e,t)=>{this.host.post(e,t).then(e=>e.json()).then(e=>{console.log(e)}).catch(e=>{console.error(e),alert(e)})},this.fetchCollection=()=>{let e=this.currentCollection?"videos/"+this.currentCollection:"collections";return this.host.get(e)},this.currentCollection=e,this.host=t,this.remote_address=s}}var l=s(9008),a=s.n(l);let h=e=>{let t=e=>{let t="w-full px-4 py-2 border-gray-200 dark:border-gray-600";return e||(t+=" border-b"),t},s=e.entry.child_collections.length-1,n=e.entry.videos.length-1,i=e.entry.child_collections.map((i,r)=>{let c=t(r==s&&n<0);return(0,o.jsx)("li",{className:c,onClick:()=>e.setCurrentCollection(i),children:i},i)}),r=e.entry.videos.map((s,i)=>{let r=t(i==n)+" text-gray-500";return(0,o.jsx)("li",{className:r,onClick:()=>{e.playVideo(s),console.log(s)},children:s},s)});return(0,o.jsxs)("ul",{className:"text-sm font-medium text-gray-900 border border-gray-200 rounded-lg dark:bg-gray-700 dark:border-gray-600 dark:text-white",children:[i,r]})},d=e=>(0,o.jsx)("button",{type:"button",className:"border rounded-full","aria-label":"Rewind 10 seconds",onClick:()=>e.onClick(),children:(0,o.jsx)("svg",{width:"72px",viewBox:"-6 -6 36 36",children:e.children})}),u=e=>{let t=e.player;return(0,o.jsxs)("footer",{className:"flex h-24 w-full items-center justify-center border-t",children:[(0,o.jsx)("div",{className:"flex-auto flex items-center justify-evenly",children:(0,o.jsxs)(d,{ariaLabel:"Rewind 10 seconds",onClick:()=>t.seek(-15),children:[(0,o.jsx)("path",{fillRule:"evenodd",clipRule:"evenodd",fillOpacity:"0.0",d:"M6.492 16.95c2.861 2.733 7.5 2.733 10.362 0 2.861-2.734 2.861-7.166 0-9.9-2.862-2.733-7.501-2.733-10.362 0A7.096 7.096 0 0 0 5.5 8.226",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round"}),(0,o.jsx)("path",{d:"M5 5v3.111c0 .491.398.889.889.889H9",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round"})]})}),(0,o.jsx)("button",{type:"button",onClick:()=>t.togglePause(),className:"bg-white text-slate-900 dark:bg-slate-100 dark:text-slate-700 flex-none -my-2 mx-auto w-20 h-20 rounded-full ring-1 ring-slate-900/5 shadow-md flex items-center justify-center","aria-label":"Pause",children:(0,o.jsxs)("svg",{width:"30",height:"32",fill:"currentColor",children:[(0,o.jsx)("rect",{x:"6",y:"4",width:"4",height:"24",rx:"2"}),(0,o.jsx)("rect",{x:"20",y:"4",width:"4",height:"24",rx:"2"})]})}),(0,o.jsx)("div",{className:"flex-auto flex items-center justify-evenly",children:(0,o.jsxs)(d,{ariaLabel:"Skip 10 seconds",onClick:()=>t.seek(15),children:[(0,o.jsx)("path",{fillOpacity:"0.0",d:"M17.509 16.95c-2.862 2.733-7.501 2.733-10.363 0-2.861-2.734-2.861-7.166 0-9.9 2.862-2.733 7.501-2.733 10.363 0 .38.365.711.759.991 1.176",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round"}),(0,o.jsx)("path",{d:"M19 5v3.111c0 .491-.398.889-.889.889H15",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round"})]})})]})},f=e=>{let[t,s]=(0,n.useState)(new r),[i,l]=(0,n.useState)(""),[d,f]=(0,n.useState)(""),p=new c(i,e.host,"");return(0,n.useEffect)(()=>{p.fetchCollection().then(e=>e.json()).then(e=>{s(e),f(e.parent_collection)}).catch(e=>{console.log(e),alert(e)})},[i]),(0,o.jsxs)("div",{className:"flex min-h-screen flex-col items-center justify-center py-2",children:[(0,o.jsxs)(a(),{children:[(0,o.jsx)("title",{children:"Videos"}),(0,o.jsx)("link",{rel:"icon",href:"/favicon.ico"})]}),(0,o.jsxs)("main",{className:"flex w-full flex-1 flex-col items-left justify-left px-2",children:[(0,o.jsx)("button",{type:"button",onClick:()=>l(d),children:"Back"}),(0,o.jsx)(h,{entry:t,setCurrentCollection:l,playVideo:p.playVideo})]}),(0,o.jsx)(u,{player:p})]})};var p=f},7498:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return a}});var o=s(5893),n=s(7294),i=s(9380),r=s(5697);class c{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.addEventListener("open",t=>{console.log("open websocket event"),void 0!==e.socket&&e.socket.send("Hello Server!"),e.open=!0}),this.socket.addEventListener("message",function(t){e.onReceive(t).catch(e=>(0,i.hV)("onReceive exception: ".concat(e)))})}},this.close=(e,t)=>{void 0!==this.socket&&(this.socket.close(e,t),this.socket=void 0,this.open=!1,this.listening=!1)},this.send=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.onReceive=async e=>{if(void 0!==this.socket&&this.open&&void 0!==this.onMessage){if(e.data instanceof r.string)(0,i.hV)(e.data);else{let t=await new Response(e.data).text();(0,i.hV)("onReceive: ".concat(t));let s=JSON.parse(t);this.onMessage(s)}}},this.isReady=()=>void 0!==this.socket&&this.open,c._instance)return c._instance;c._instance=this,c.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}let l=e=>{let[t,s]=(0,n.useState)(""),r=(0,n.useRef)(null);(0,n.useEffect)(()=>{new c(()=>new WebSocket("ws://".concat(e.host.getHost(),"/ws")),t=>{if(void 0!==t.Play){let o="http://".concat(e.host.getHost()).concat(t.Play.url);s(o)}else if(void 0!==t.Seek&&null!==r){let e=r.current;e.currentTime=e.currentTime+t.Seek.interval}else if(void 0!==t.TogglePause&&null!==r){let e=r.current;e.paused?e.play().catch(e=>{(0,i.FO)(e.message)}):e.pause()}})},[e.host]);let l=e=>{let t=e.currentTarget;t.requestFullscreen().then(e=>(0,i.hV)("requestFullscreen: ".concat(e))).catch(e=>(0,i.tu)("failed: ".concat(e))),t.className="w-full"},a=e=>{(0,i.hV)("getNextVideo called")};return""!=t?(0,o.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,o.jsx)("video",{className:"h-screen m-auto",onLoadedMetadata:e=>l(e),onEnded:e=>a(e),style:{objectFit:"contain"},id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:t,ref:r})}):(0,o.jsx)("p",{children:"Waiting for video to be selected"})};var a=l},8655:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return u}});var o,n,i=s(5893),r=s(7498),c=s(9380),l=s(830),a=s(7294);let h=new class{constructor(e){this.getHost=()=>this.host,this.get=async e=>fetch(this.makeUrl(e)),this.post=async(e,t)=>{let s={method:"POST",body:JSON.stringify(t),headers:{"Content-Type":"application/json"}};return fetch(this.makeUrl(e),s)},this.makeUrl=e=>"http://".concat(this.host,"/").concat(e),this.host=e}}("coco.abamaxa.com");(0,c.hu)(h),(o=n||(n={}))[o.Unknown=1]="Unknown",o[o.Video=2]="Video",o[o.Remote=3]="Remote";let d=()=>{let[e,t]=(0,a.useState)(n.Unknown);return((0,a.useEffect)(()=>{let e=window.navigator.userAgent;e.includes("SMART-TV")||e.includes("Firefox/109")?t(n.Video):t(n.Remote)},[]),e==n.Video)?(0,i.jsx)(r.default,{host:h}):e==n.Remote?(0,i.jsx)(l.default,{host:h}):(0,i.jsx)("p",{children:"Loading..."})};var u=d},9380:function(e,t,s){"use strict";s.d(t,{FO:function(){return l},hV:function(){return c},hu:function(){return i},tu:function(){return a}});class o{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}let n=null,i=e=>{n=new o(e)},r=(e,t)=>{null===n?console.log("NO LOGGER: ".concat(e," - ").concat(t)):(console.log("calling logger with: ".concat(e," - ").concat(t)),n.log(e,t))},c=e=>{r("info",e)},l=e=>{r("warning",e)},a=e=>{let t=Error().stack;if(void 0===t||null==n)r("error",e);else{let s=t.split("\n");n.log_messages("error",[e,...s])}}},9008:function(e,t,s){e.exports=s(3121)},2703:function(e,t,s){"use strict";var o=s(414);function n(){}function i(){}i.resetWarningCache=n,e.exports=function(){function e(e,t,s,n,i,r){if(r!==o){var c=Error("Calling PropTypes validators directly is not supported by the `prop-types` package. Use PropTypes.checkPropTypes() to call them. Read more at http://fb.me/use-check-prop-types");throw c.name="Invariant Violation",c}}function t(){return e}e.isRequired=e;var s={array:e,bigint:e,bool:e,func:e,number:e,object:e,string:e,symbol:e,any:e,arrayOf:t,element:e,elementType:e,instanceOf:t,node:e,objectOf:t,oneOf:t,oneOfType:t,shape:t,exact:t,checkPropTypes:i,resetWarningCache:n};return s.PropTypes=s,s}},5697:function(e,t,s){e.exports=s(2703)()},414:function(e){"use strict";e.exports="SECRET_DO_NOT_PASS_THIS_OR_YOU_WILL_BE_FIRED"}},function(e){e.O(0,[774,888,179],function(){return e(e.s=8312)}),_N_E=e.O()}]);