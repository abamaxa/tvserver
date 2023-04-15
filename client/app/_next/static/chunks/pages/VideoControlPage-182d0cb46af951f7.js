(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[614],{6276:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/VideoControlPage",function(){return s(6877)}])},6877:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return l}});var n=s(5893),i=s(7294),o=s(9518),r=s(5697);class c{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.addEventListener("open",t=>{console.log("open websocket event"),void 0!==e.socket&&e.socket.send("Hello Server!"),e.open=!0}),this.socket.addEventListener("message",function(t){e.onReceive(t).catch(e=>(0,o.hV)("onReceive exception: ".concat(e)))})}},this.close=(e,t)=>{void 0!==this.socket&&(this.socket.close(e,t),this.socket=void 0,this.open=!1,this.listening=!1)},this.send=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.onReceive=async e=>{void 0!==this.socket&&this.open&&void 0!==this.onMessage&&(e.data instanceof r.string?(0,o.hV)(e.data):await this._parseMessage(e))},this._parseMessage=async e=>{try{let t=await new Response(e.data).text();(0,o.hV)("onReceive: ".concat(t));let s=JSON.parse(t);this.onMessage(s)}catch(t){(0,o.tu)("error ".concat(t,", received unexpected message: ").concat(e.data))}},this.isReady=()=>void 0!==this.socket&&this.open,c._instance)return c._instance;c._instance=this,c.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}let a=e=>{let[t,s]=(0,i.useState)(""),r=(0,i.useRef)(null);(0,i.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host;new c(()=>new WebSocket("ws://".concat(t,"/remote/ws")),e=>{if(void 0!==e.Play)s(e.Play.url);else if(void 0!==e.Seek&&null!==r){let t=r.current;t.currentTime=t.currentTime+e.Seek.interval}else if(void 0!==e.TogglePause&&null!==r){let e=r.current;a(e)}})},[e.host]);let a=e=>{e.paused?e.play().catch(e=>{(0,o.FO)(e.message)}):e.pause()},l=e=>{(0,o.hV)("getNextVideo called")},h=e=>{let t=e.target.error;t&&(0,o.hV)("Video error: ".concat(t.message))};return""!=t?(0,n.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,n.jsx)("video",{className:"m-auto w-full h-screen object-contain outline-0",onEnded:e=>l(e),onError:e=>h(e),id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:t,ref:r})}):(0,n.jsx)("h1",{className:"text-6xl text-white bg-black text-center h-screen py-32",children:"Ready"})};var l=a},9116:function(e,t,s){"use strict";s.d(t,{No:function(){return l},av:function(){return u},bZ:function(){return v},jt:function(){return f},jx:function(){return h},w8:function(){return d}});var n,i,o=s(5893),r=s(765),c=s(3854),a=s(9518);(n=i||(i={}))[n.Error=0]="Error",n[n.Information=1]="Information",n[n.Warning=2]="Warning",n[n.Question=3]="Question";let l=new class{constructor(){this.message="",this.show=!1,this.type=i.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=(0,a.HG)(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{let e=this.onOk;this.onOk=void 0,this.hideAlert(),void 0!==e&&e()}}},h=e=>{g(e,i.Error)},u=e=>{g(e,i.Warning)},d=e=>{g(e,i.Information)},f=(e,t)=>{g(e,i.Question,t)},g=(e,t,s)=>{if(void 0!==l)l.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},v=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",n=null;return l.type===i.Information?t=(0,o.jsx)(c.if7,{className:s+"text-gray-400 dark:text-gray-200"}):l.type===i.Error?t=(0,o.jsx)(c.baL,{className:s+"text-red-400 dark:text-red-200"}):l.type===i.Question?(t=(0,o.jsx)(c.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),n=(0,o.jsx)(r.zx,{outline:!0,onClick:()=>l.hideAlert(),children:"Cancel"})):t=(0,o.jsx)(c.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,o.jsxs)(r.u_,{className:"z-50",show:e.show,size:"md",popup:!0,children:[(0,o.jsx)(r.u_.Header,{}),(0,o.jsx)(r.u_.Body,{children:(0,o.jsxs)("div",{className:"text-center",children:[t,(0,o.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:l.message}),(0,o.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,o.jsx)(r.zx,{onClick:()=>l.okClicked(),children:"Ok"}),n]})]})})]})}},9518:function(e,t,s){"use strict";s.d(t,{FO:function(){return l},HG:function(){return u},hV:function(){return a},hu:function(){return r},tu:function(){return h}});var n=s(9116);class i{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}let o=null,r=e=>{o=new i(e)},c=(e,t,s)=>{let n=s?console.error:console.log,i=u(t);if(n("".concat(e," - ").concat(i)),null!==o){let t=[i];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}o.log_messages(e,t)}},a=e=>{c("info",e)},l=e=>{c("warning",e),(0,n.av)(e)},h=e=>{c("error",e,!0),(0,n.jx)(e)},u=e=>"string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e)}},function(e){e.O(0,[556,935,774,888,179],function(){return e(e.s=6276)}),_N_E=e.O()}]);