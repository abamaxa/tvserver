"use strict";(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[52],{5052:function(e,t,s){s.r(t);var i=s(5893),n=s(7294),o=s(9518),r=s(7091),c=s(9098);let a=e=>{let[t,s]=(0,n.useState)(null),[a,l]=(0,n.useState)(),h=(0,n.useRef)(null);(0,n.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host,i=new r.Z(()=>new WebSocket("ws://".concat(t,"/api/remote/ws")),e=>{if(void 0!==e.Play)l(e.Play);else if(void 0!==e.Seek&&null!==h){let t=h.current;t.currentTime=t.currentTime+e.Seek.interval}else if(void 0!==e.TogglePause&&null!==h){let e=h.current;u(e)}});s(i)},[e.host]);let u=e=>{e.paused?e.play().catch(e=>{(0,o.FO)(e.message)}):e.pause()},d=e=>{(0,o.hV)("getNextVideo called")},f=e=>{let t=e.target.error;t&&(0,o.hV)("Video error: ".concat(t.message))},g=e=>{if(null!==t&&void 0!==a){let s=e.currentTarget;t.send({State:new c.cp(s,a.collection,a.video)})}};return void 0!==a?(0,i.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,i.jsx)("video",{className:"m-auto w-full h-screen object-contain outline-0",onEnded:e=>d(e),onError:e=>f(e),onTimeUpdate:e=>g(e),id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:a.url,ref:h})}):(0,i.jsx)("h1",{className:"text-4xl text-white bg-black text-center h-screen py-32",children:"Something To See Coming Soon..."})};t.default=a},7091:function(e,t,s){s.d(t,{Z:function(){return o}});var i=s(9518),n=s(5697);class o{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.addEventListener("open",t=>{console.log("open websocket event"),void 0!==e.socket&&e.socket.send("Hello Server!"),e.open=!0}),this.socket.addEventListener("message",function(t){e.onReceive(t).catch(e=>(0,i.hV)("onReceive exception: ".concat(e)))})}},this.close=(e,t)=>{void 0!==this.socket&&(this.socket.close(e,t),this.socket=void 0,this.open=!1,this.listening=!1)},this.send_string=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.send=e=>{if(void 0!==this.socket&&this.open){let t=JSON.stringify(e),s=new Blob([t],{type:"application/json"});try{this.socket.send(s)}catch(e){this._reconnect(),this.socket.send(s)}}},this.onReceive=async e=>{void 0!==this.socket&&this.open&&void 0!==this.onMessage&&(e.data instanceof n.string?(0,i.hV)(e.data):await this._parseMessage(e))},this._parseMessage=async e=>{try{let t=await new Response(e.data).text(),s=JSON.parse(t);this.onMessage(s)}catch(t){(0,i.tu)("error ".concat(t,", received unexpected message: ").concat(e.data))}},this.isReady=()=>void 0!==this.socket&&this.open,this._reconnect=()=>{o.socketBuilder&&(this.close(),this.socket=o.socketBuilder(),this.addListeners())},o._instance)return o._instance;o._instance=this,o.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}},9116:function(e,t,s){s.d(t,{No:function(){return l},av:function(){return u},bZ:function(){return m},jt:function(){return f},jx:function(){return h},w8:function(){return d}});var i,n,o=s(5893),r=s(8640),c=s(3854),a=s(9518);(i=n||(n={}))[i.Error=0]="Error",i[i.Information=1]="Information",i[i.Warning=2]="Warning",i[i.Question=3]="Question";let l=new class{constructor(){this.message="",this.show=!1,this.type=n.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=(0,a.HG)(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{let e=this.onOk;this.onOk=void 0,this.hideAlert(),void 0!==e&&e()}}},h=e=>{g(e,n.Error)},u=e=>{g(e,n.Warning)},d=e=>{g(e,n.Information)},f=(e,t)=>{g(e,n.Question,t)},g=(e,t,s)=>{if(void 0!==l)l.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},m=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",i=null;return l.type===n.Information?t=(0,o.jsx)(c.if7,{className:s+"text-gray-400 dark:text-gray-200"}):l.type===n.Error?t=(0,o.jsx)(c.baL,{className:s+"text-red-400 dark:text-red-200"}):l.type===n.Question?(t=(0,o.jsx)(c.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),i=(0,o.jsx)(r.zx,{outline:!0,onClick:()=>l.hideAlert(),children:"Cancel"})):t=(0,o.jsx)(c.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,o.jsxs)(r.u_,{className:"z-50",show:e.show,size:"md",popup:!0,children:[(0,o.jsx)(r.u_.Header,{}),(0,o.jsx)(r.u_.Body,{children:(0,o.jsxs)("div",{className:"text-center",children:[t,(0,o.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:l.message}),(0,o.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,o.jsx)(r.zx,{onClick:()=>l.okClicked(),children:"Ok"}),i]})]})})]})}},9098:function(e,t,s){var i,n,o,r;s.d(t,{HJ:function(){return h},_0:function(){return u},a7:function(){return a},cp:function(){return c},hk:function(){return l}});class c{constructor(e,t,s){this.currentTime=e.currentTime,this.duration=e.duration,this.currentSrc=e.currentSrc,this.collection=t,this.video=s}}class a{constructor(e,t){this.remote_address="",this.message=e,void 0!==t&&(this.remote_address=t)}}class l{constructor(){this.collection="",this.parent_collection="",this.child_collections=[],this.videos=[],this.errors=[]}}(o=i||(i={})).YouTube="youtube",o.PirateBay="piratebay",(r=n||(n={})).Transmission="transmission",r.AsyncProcess="asyncprocess";class h{constructor(e){this.name=e}}class u{constructor(e){this.newName=e}}},9518:function(e,t,s){s.d(t,{FO:function(){return l},HG:function(){return u},hV:function(){return a},hu:function(){return r},tu:function(){return h}});var i=s(9116);class n{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}let o=null,r=e=>{o=new n(e)},c=(e,t,s)=>{let i=s?console.error:console.log,n=u(t);if(i("".concat(e," - ").concat(n)),null!==o){let t=[n];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}o.log_messages(e,t)}},a=e=>{c("info",e)},l=e=>{c("warning",e),(0,i.av)(e)},h=e=>{c("error",e,!0),(0,i.jx)(e)},u=e=>"string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e)}}]);