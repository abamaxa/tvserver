(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[614],{6276:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/VideoControlPage",function(){return s(7498)}])},7498:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return a}});var n=s(5893),i=s(7294),o=s(9380),r=s(5697);class c{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.addEventListener("open",t=>{console.log("open websocket event"),void 0!==e.socket&&e.socket.send("Hello Server!"),e.open=!0}),this.socket.addEventListener("message",function(t){e.onReceive(t).catch(e=>(0,o.hV)("onReceive exception: ".concat(e)))})}},this.close=(e,t)=>{void 0!==this.socket&&(this.socket.close(e,t),this.socket=void 0,this.open=!1,this.listening=!1)},this.send=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.onReceive=async e=>{if(void 0!==this.socket&&this.open&&void 0!==this.onMessage){if(e.data instanceof r.string)(0,o.hV)(e.data);else{let t=await new Response(e.data).text();(0,o.hV)("onReceive: ".concat(t));let s=JSON.parse(t);this.onMessage(s)}}},this.isReady=()=>void 0!==this.socket&&this.open,c._instance)return c._instance;c._instance=this,c.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}let l=e=>{let[t,s]=(0,i.useState)(""),r=(0,i.useRef)(null);(0,i.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host;new c(()=>new WebSocket("ws://".concat(t,"/remote/ws")),e=>{if(void 0!==e.Play)s(e.Play.url);else if(void 0!==e.Seek&&null!==r){let t=r.current;t.currentTime=t.currentTime+e.Seek.interval}else if(void 0!==e.TogglePause&&null!==r){let e=r.current;e.paused?e.play().catch(e=>{(0,o.FO)(e.message)}):e.pause()}})},[e.host]);let l=e=>{let t=e.currentTarget;t.requestFullscreen().then(e=>(0,o.hV)("requestFullscreen: ".concat(e))).catch(e=>(0,o.hV)("failed: ".concat(e))),t.className="w-full"},a=e=>{(0,o.hV)("getNextVideo called")};return""!=t?(0,n.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,n.jsx)("video",{className:"h-screen m-auto",onLoadedMetadata:e=>l(e),onEnded:e=>a(e),style:{objectFit:"contain"},id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:t,ref:r})}):(0,n.jsx)("p",{children:"Waiting for video to be selected"})};var a=l},9116:function(e,t,s){"use strict";s.d(t,{LK:function(){return f},No:function(){return a},av:function(){return u},bZ:function(){return v},jx:function(){return h},w8:function(){return d}});var n,i,o=s(5893),r=s(765),c=s(3854),l=s(9380);(n=i||(i={}))[n.Error=0]="Error",n[n.Information=1]="Information",n[n.Warning=2]="Warning",n[n.Question=3]="Question";let a=new class{constructor(){this.message="",this.show=!1,this.type=i.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=(0,l.HG)(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{void 0!==this.onOk&&(this.onOk(),this.onOk=void 0),this.hideAlert()}}},h=e=>{g(e,i.Error)},u=e=>{g(e,i.Warning)},d=e=>{g(e,i.Information)},f=(e,t)=>{g(e,i.Question,t)},g=(e,t,s)=>{if(void 0!==a)a.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},v=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",n=null;return a.type===i.Information?t=(0,o.jsx)(c.if7,{className:s+"text-gray-400 dark:text-gray-200"}):a.type===i.Error?t=(0,o.jsx)(c.baL,{className:s+"text-red-400 dark:text-red-200"}):a.type===i.Question?(t=(0,o.jsx)(c.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),n=(0,o.jsx)(r.zx,{outline:!0,onClick:()=>a.hideAlert(),children:"Cancel"})):t=(0,o.jsx)(c.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,o.jsxs)(r.u_,{show:e.show,size:"md",popup:!0,children:[(0,o.jsx)(r.u_.Header,{}),(0,o.jsx)(r.u_.Body,{children:(0,o.jsxs)("div",{className:"text-center",children:[t,(0,o.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:a.message}),(0,o.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,o.jsx)(r.zx,{onClick:()=>a.okClicked(),children:"Ok"}),n]})]})})]})}},9380:function(e,t,s){"use strict";s.d(t,{FO:function(){return a},HG:function(){return u},hV:function(){return l},hu:function(){return r},tu:function(){return h}});var n=s(9116);class i{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}let o=null,r=e=>{o=new i(e)},c=(e,t,s)=>{let n=s?console.error:console.log,i=u(t);if(null===o)n("NO LOGGER: ".concat(e," - ").concat(i));else{n("".concat(e," - ").concat(i));let t=[i];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}o.log_messages(e,t)}},l=e=>{c("info",e)},a=e=>{c("warning",e),(0,n.av)(e)},h=e=>{c("error",e,!0),(0,n.jx)(e)},u=e=>"string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e)}},function(e){e.O(0,[556,935,774,888,179],function(){return e(e.s=6276)}),_N_E=e.O()}]);