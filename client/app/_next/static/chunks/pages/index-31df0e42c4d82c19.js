(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[405],{8312:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/",function(){return s(9152)}])},9152:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return eB}});var a,i,r,o,l,n,c,d,h,u,m,x,g,v,p,y,f=s(5893),j=s(7294),k=s(8640),w=s(3854);(a=h||(h={}))[a.Error=0]="Error",a[a.Information=1]="Information",a[a.Warning=2]="Warning",a[a.Question=3]="Question";let b=new class{constructor(){this.message="",this.show=!1,this.type=h.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=O(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{let e=this.onOk;this.onOk=void 0,this.hideAlert(),void 0!==e&&e()}}},C=e=>{E(e,h.Error)},N=e=>{E(e,h.Warning)},S=e=>{E(e,h.Information)},T=(e,t)=>{E(e,h.Question,t)},E=(e,t,s)=>{if(void 0!==b)b.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},D=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",a=null;return b.type===h.Information?t=(0,f.jsx)(w.if7,{className:s+"text-gray-400 dark:text-gray-200"}):b.type===h.Error?t=(0,f.jsx)(w.baL,{className:s+"text-red-400 dark:text-red-200"}):b.type===h.Question?(t=(0,f.jsx)(w.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),a=(0,f.jsx)(k.zx,{outline:!0,onClick:()=>b.hideAlert(),children:"Cancel"})):t=(0,f.jsx)(w.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,f.jsxs)(k.u_,{className:"z-50",show:e.show,size:"md",popup:!0,children:[(0,f.jsx)(k.u_.Header,{}),(0,f.jsx)(k.u_.Body,{children:(0,f.jsxs)("div",{className:"text-center",children:[t,(0,f.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:b.message}),(0,f.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,f.jsx)(k.zx,{onClick:()=>b.okClicked(),children:"Ok"}),a]})]})})]})},_=null,V=(e,t,s,a)=>{let i=s?console.error:console.log,r=O(t,a);if(i("".concat(e," - ").concat(r)),null!==_){let t=[r];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}_.log_messages(e,t)}},R=e=>{V("info",e)},A=e=>{V("warning",e),N(e)},P=(e,t)=>{V("error",e,!0,t),C(e)},O=(e,t)=>{let s;return(s="string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e),void 0!==t)?"".concat(t,": ").concat(s):s};var L=s(5697);class I{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.onopen=()=>{console.log("open websocket event"),void 0!==e.socket&&(e.open=!0,e.send({SendLastState:null}))},this.socket.onerror=t=>{console.log("WebSocket encountered error: ".concat(JSON.stringify(t),", closing socket")),setTimeout(e._reconnect,1e3)},this.socket.onmessage=t=>{e.onReceive(t).catch(e=>R("onReceive exception: ".concat(e)))}}},this.close=(e,t)=>{if(void 0!==this.socket){try{this.socket.close(e,t)}catch(e){}this.socket=void 0,this.open=!1,this.listening=!1}},this.send_string=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.send=e=>{if(void 0!==this.socket&&this.open){let t=JSON.stringify(e),s=new Blob([t],{type:"application/json"});this.socket.readyState>1&&this._reconnect();try{this.socket.send(s)}catch(e){this._reconnect(),this.socket.send(s)}}},this.onReceive=async e=>{void 0!==this.socket&&this.open&&void 0!==this.onMessage&&(e.data instanceof L.string?R(e.data):await this._parseMessage(e))},this._parseMessage=async e=>{try{let t=await new Response(e.data).text(),s=JSON.parse(t);this.onMessage(s)}catch(t){P("error ".concat(t,", received unexpected message: ").concat(e.data))}},this.isReady=()=>void 0!==this.socket&&this.open,this._reconnect=()=>{if(I.socketBuilder)try{this.close(),this.socket=I.socketBuilder(),this.addListeners()}catch(e){console.log("reconnecting websocket: ".concat(e))}},I._instance)return I._instance;I._instance=this,I.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}class M{constructor(e,t,s){this.currentTime=e.currentTime,this.duration=e.duration,this.currentSrc=e.currentSrc,this.collection=t,this.video=s}}class U{constructor(e,t){this.remote_address="",this.message=e,void 0!==t&&(this.remote_address=t)}}class F{constructor(){this.collection="",this.parent_collection="",this.child_collections=[],this.videos=[],this.errors=[]}}(i=u||(u={})).YouTube="youtube",i.PirateBay="piratebay",(r=m||(m={})).Transmission="transmission",r.AsyncProcess="asyncprocess";class z{constructor(e){this.name=e}}class B{constructor(e){this.newName=e}}let H="text-sm font-medium text-gray-900 border border-gray-200 rounded-lg bg-white dark:bg-gray-700 dark:border-gray-600 dark:text-white",K="w-full px-4 py-2 border-gray-200 dark:border-gray-600 overflow-hidden",W="piratebay",Y="youtube";(o=x||(x={}))[o.OK=200]="OK",o[o.ACCEPTED=202]="ACCEPTED",o[o.NO_CONTENT=204]="NO_CONTENT",o[o.PARTIAL_CONTENT=206]="PARTIAL_CONTENT",o[o.BAD_REQUEST=400]="BAD_REQUEST",o[o.UNAUTHORIZED=401]="UNAUTHORIZED",o[o.FORBIDDEN=403]="FORBIDDEN",o[o.NOT_FOUND=404]="NOT_FOUND",o[o.CONFLICT=409]="CONFLICT",o[o.UNPROCESSABLE_ENTITY=422]="UNPROCESSABLE_ENTITY",o[o.INTERNAL_SERVER_ERROR=500]="INTERNAL_SERVER_ERROR",o[o.NOT_IMPLEMENTED=501]="NOT_IMPLEMENTED";let q=e=>{let t=J(e.isLast)+" text-gray-600",s=e.name,a=()=>{e.setVideoDetails(e.name)};return(0,f.jsx)("li",{className:t,onClick:e=>a(),children:(0,f.jsx)("div",{children:s})})},J=e=>{let t=K;return e||(t+=" border-b"),t},Q=e=>{let t=e.entry.child_collections.length-1,s=e.entry.videos.length-1,a=(()=>{let t=X(!1),s=e.entry.parent_collection;return""===e.entry.collection?(0,f.jsx)(f.Fragment,{}):(0,f.jsxs)("li",{className:t,onClick:()=>e.setCurrentCollection(s),children:["<-"," Back"]},"0")})(),i=e.entry.child_collections.map((a,i)=>(0,f.jsx)(G,{isLast:i===t&&s<0,name:a,parent:e.entry.collection,setCurrentCollection:e.setCurrentCollection},a)),r=e.entry.videos.map((t,a)=>{let i=t.Video;return void 0===i?null:(0,f.jsx)(q,{isLast:a===s,name:i.video,videoPlayer:e.videoPlayer,setVideoDetails:e.setVideoDetails},a)});return(0,f.jsx)(f.Fragment,{children:(0,f.jsxs)("ul",{className:H,children:[a,i,r]})})},G=e=>{let t=X(e.isLast),s=""!==e.parent?e.parent+"/"+e.name:e.name;return(0,f.jsx)("li",{className:t,onClick:()=>e.setCurrentCollection(s),children:e.name})},X=e=>{let t=K;return e||(t+=" border-b"),t},Z=e=>{let[t,s]=(0,j.useState)(new F);return(0,j.useEffect)(()=>{let t=async()=>{try{let t=await e.videoPlayer.fetchDetails();t.Collection&&s(t.Collection)}catch(e){P(e,"fetchDetails")}};if(e.isActive){t();let e=setInterval(async()=>{await t()},2e5);return()=>clearInterval(e)}},[e.videoPlayer.getCurrentCollection,e.videoPlayer,e.isActive]),(0,f.jsx)("div",{className:"flex min-h-full flex-col items-center justify-center p-0",children:(0,f.jsx)("main",{className:"flex w-full flex-1 flex-col items-left justify-left",children:(0,f.jsx)(Q,{entry:t,setCurrentCollection:e.videoPlayer.setCurrentCollection,setVideoDetails:e.showVideoDetails,videoPlayer:e.videoPlayer})})})};class ${constructor(e,t,s,a){this.getAvailableConversions=async()=>{let e=[];try{let t=await this.host.get("conversion");t.error&&P("conversions are unavailable: "+t.error),e=t.results||[]}catch(e){P(e,"VideoPlayer.getAvailableConversions")}return e},this.deleteVideo=async(e,t)=>{let s=[x.OK,x.ACCEPTED,x.NO_CONTENT];T('Delete video "'.concat(e,'?"'),async()=>{try{let a=this.makePath(e),i=await this.host.delete("media/".concat(a));-1===s.findIndex(e=>e===i.status)?P('cannot delete "'.concat(e,'": "').concat(i.statusText,'"')):void 0!==t&&t()}catch(t){P(t,"deleteVideo: ".concat(e))}})},this.renameVideo=async(e,t)=>{T('rename video "'.concat(e,'" to "').concat(t,'"?'),async()=>{try{let s=this.makePath(e),a=this.makePath(t),i=await this.host.put("media/".concat(s),new B(a));i.status!==x.OK&&P('cannot rename "'.concat(e,'" to "').concat(t,'": "').concat(i.statusText,'"'))}catch(s){P(s,'renameVideo: "'.concat(e,'" to "').concat(t,'"'))}})},this.convertVideo=async(e,t)=>{T("".concat(t,' with "').concat(e,'"?'),async()=>{try{let s=this.makePath(e),a=await this.host.post("media/".concat(s),new z(t));a.status!==x.OK&&P('cannot convert "'.concat(e,'": "').concat(a.statusText,'"'))}catch(s){P(s,"convertVideo: ".concat(t,' with "').concat(e,'"'))}})},this.setCurrentCollection=e=>{this.setCurrentCollectionHook(e)},this.getCurrentCollection=()=>this.currentCollection,this.playVideo=e=>{let t={remote_address:this.remote_address,collection:this.currentCollection,video:e};this.post("remote/play",t)},this.seek=e=>{let t=new U({Seek:{interval:e}});this.post("remote/control",t)},this.togglePause=()=>{let e=new U({TogglePause:"ok"});this.post("remote/control",e)},this.post=async(e,t)=>{try{let s=await this.host.post(e,t);if(s.status!==x.OK){S(s.statusText);return}let a=await s.json();a.errors.length>0&&S(a.errors[0])}catch(t){P(t,"post: ".concat(e))}},this.fetchDetails=async(e,t)=>{let s="media";return void 0!==t&&t?s+="/"+t:this.currentCollection&&(s+="/"+this.currentCollection),void 0!==e&&e&&(s+="/"+e),await this.host.get(s)},this.makePath=e=>this.currentCollection?"".concat(this.currentCollection,"/").concat(e):e,this.currentCollection=e,this.host=s,this.remote_address=a,this.setCurrentCollectionHook=t}}(l=g||(g={}))[l.STOPPED=1]="STOPPED",l[l.PAUSED=2]="PAUSED",l[l.STARTED=3]="STARTED";let ee=(0,j.forwardRef)((e,t)=>{let s=(0,j.useRef)(null);(0,j.useImperativeHandle)(t,()=>{let e=s.current;return{play:()=>{e.play()},pause:()=>{e.paused?e.play().catch(e=>{A(e.message)}):e.pause()},stop:()=>{e&&(e.pause(),e.currentTime=0)},seek:t=>{e&&(e.currentTime=e.currentTime+t)}}});let a=e=>{R("getNextVideo called")},i=e=>{let t=e.target.error;t&&R("Video error: ".concat(t.message))},r=e=>{e.preventDefault(),R("keyPress: ".concat(e.key,", code: ").concat(e.code))},o=t=>{if(null!==e.socket&&void 0!==e.currentVideo){let s=t.currentTarget;e.socket.send({State:new M(s,e.currentVideo.collection,e.currentVideo.video)})}};return(0,f.jsx)("div",{className:"bg-black h-screen w-screen absolute",children:(0,f.jsx)("video",{className:"m-auto w-full h-screen object-contain outline-0",onEnded:e=>a(e),onError:e=>i(e),onTimeUpdate:e=>o(e),onKeyDown:e=>r(e),onKeyDownCapture:e=>r(e),onEndedCapture:t=>e.onStateChange(g.STOPPED),onPause:t=>e.onStateChange(g.PAUSED),onPlayCapture:t=>e.onStateChange(g.STARTED),id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:e.currentVideo.url,ref:s})})}),et=e=>{let[t,s]=(0,j.useState)(null),[a,i]=(0,j.useState)(!0),[r,o]=(0,j.useState)(),[l,n]=(0,j.useState)(""),c=(0,j.useRef)(null),d=new $(l,n,e.host,"");(0,j.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host,a=new I(()=>new WebSocket("ws://".concat(t,"/api/remote/ws")),e=>{var t,s;void 0!==e.Play?o(e.Play):void 0!==e.Seek&&null!==c?null===(t=c.current)||void 0===t||t.seek(e.Seek.interval):void 0!==e.TogglePause&&null!==c&&(null===(s=c.current)||void 0===s||s.pause())});s(a)},[e.host]);let h=e=>{console.log("video: ".concat(e)),d.playVideo(e)},u=e=>{e===g.STARTED?i(!1):i(!0)},m=(0,f.jsx)(f.Fragment,{}),x=(0,f.jsx)(f.Fragment,{});return void 0!==r&&(m=(0,f.jsx)(ee,{currentVideo:r,socket:t,ref:c,onStateChange:u})),console.log("render: ".concat(a)),a&&(x=(0,f.jsx)("div",{className:"p-1 mx-auto overflow-y-auto w-1/2 z-10",children:(0,f.jsx)(Z,{videoPlayer:d,isActive:!0,showVideoDetails:h})})),(0,f.jsxs)("div",{className:"bg-black h-screen w-screen flex",children:[m,x]})};(n=v||(v={})).TERM="TERM",n.RESULTS="RESULTS",n.ENGINE="ENGINE",n.LAST_SEARCH="LAST_SEARCH";let es=(e,t)=>{let{type:s,payload:a}=t;switch(s){case v.TERM:return{...e,term:a};case v.ENGINE:return{...e,engine:a};case v.RESULTS:return{...e,results:a};case v.LAST_SEARCH:return{...e,lastSearch:a};default:return e}},ea=e=>{let t=t=>{let s=t.currentTarget;e.dispatch({type:v.ENGINE,payload:s.value})},s=t=>{e.dispatch({type:v.TERM,payload:t.currentTarget.value})},a=t=>{e.state.term&&e.doSearch(e.state.term,e.state.engine),t.preventDefault()};return(0,f.jsxs)("form",{className:"items-center",onSubmit:e=>a(e),children:[(0,f.jsxs)("div",{className:"flex w-full",children:[(0,f.jsx)("label",{htmlFor:"simple-search",className:"sr-only",children:"SearchTab"}),(0,f.jsxs)("div",{className:"relative w-full",children:[(0,f.jsx)("div",{className:"absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none",children:(0,f.jsx)("svg",{"aria-hidden":"true",className:"w-5 h-5 text-gray-500 dark:text-gray-400",fill:"currentColor",viewBox:"0 0 20 20",xmlns:"http://www.w3.org/2000/svg",children:(0,f.jsx)("path",{fillRule:"evenodd",d:"M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z",clipRule:"evenodd"})})}),(0,f.jsx)("input",{type:"text",id:"search-term",onChange:s,value:e.state.term,className:"bg-gray-50 border border-gray-300 text-gray-900 text-sm rounded-lg focus:ring-blue-500 focus:border-blue-500 block w-full pl-10 p-2.5  dark:bg-gray-700 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white dark:focus:ring-blue-500 dark:focus:border-blue-500",placeholder:"SearchTab",required:!0})]}),(0,f.jsxs)("button",{type:"submit",className:"p-2.5 ml-2 text-sm font-medium text-white bg-blue-700 rounded-lg border border-blue-700 hover:bg-blue-800 focus:ring-4 focus:outline-none focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800",children:[(0,f.jsx)("svg",{className:"w-5 h-5",fill:"none",stroke:"currentColor",viewBox:"0 0 24 24",xmlns:"http://www.w3.org/2000/svg",children:(0,f.jsx)("path",{strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"2",d:"M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"})}),(0,f.jsx)("span",{className:"sr-only",children:"SearchTab"})]})]}),(0,f.jsxs)("div",{className:"flex gap-4 pt-4 px-1",children:[(0,f.jsxs)("div",{className:"flex items-center gap-2",children:[(0,f.jsx)(k.Y8,{id:"piratebay",name:"engine",value:W,defaultChecked:e.state.engine===W,onChange:t}),(0,f.jsx)(k.__,{htmlFor:"piratebay",children:"PirateBay"})]}),(0,f.jsxs)("div",{className:"flex items-center gap-2",children:[(0,f.jsx)(k.Y8,{id:"youtube",name:"engine",value:Y,defaultChecked:e.state.engine===Y,onChange:t}),(0,f.jsx)(k.__,{htmlFor:"youtube",children:"YouTube"})]})]})]})},ei=H+" mt-4",er=e=>{let t=e.results.map((t,s,a)=>{let i=K;return s!=a.length-1&&(i+=" border-b"),(0,f.jsxs)("li",{className:i,onClick:s=>e.onItemClick(t),children:[(0,f.jsx)("p",{children:t.title}),(0,f.jsx)("p",{className:"text-gray-600 text-xs",children:t.description})]},"search:"+s)});if(0!=e.results.length)return(0,f.jsx)("ul",{className:ei,children:t});{let t=e.state.lastSearch?"for ".concat(e.state.lastSearch):"";return(0,f.jsxs)("p",{className:"px-1 py-2",children:["No results ",t]})}};class eo{async query(e,t){try{let s=await this.host.get(this.url+e);null!==s.results?t(s.results):null!==s.error&&S(s.error)}catch(t){P(t,"query: ".concat(e))}}constructor(e,t){this.host=e,this.url=t}}class el extends eo{constructor(e){super(e,"search/pirate?q=")}}class en extends eo{constructor(e){super(e,"search/youtube?q=")}}class ec{async list(e){try{let t=await this.host.get("tasks");null!==t.results?e(t.results):null!==t.error&&A(t.error)}catch(e){P(e,"list")}}async add(e){try{await this.host.post("tasks",{name:e.title,link:e.link,engine:e.engine})}catch(e){P(e,"TaskManager.add")}}async delete(e){let t=[x.OK,x.ACCEPTED,x.NO_CONTENT];T('Terminate task "'.concat(e.name,'?"'),async()=>{try{let s=await this.host.delete("tasks/".concat(e.taskType,"/").concat(e.key));-1===t.findIndex(e=>e===s.status)&&P('cannot terminate task "'.concat(e.name,'": "').concat(s.statusText,'"'))}catch(e){P(e,"TaskManager.delete")}})}constructor(e){this.host=e}}let ed=e=>{let t=e.state.results,s=t=>{e.dispatch({type:v.LAST_SEARCH,payload:e.state.term}),e.dispatch({type:v.RESULTS,payload:t})},a=t=>{T("Download ".concat(t.title,"?"),async()=>{let s=new ec(e.host);await s.add(t)})},i=(t,a)=>{let i;switch(a){case"piratebay":i=new el(e.host);break;case"youtube":i=new en(e.host);break;default:P("unknown search engine ".concat(a));return}i.query(t,s)};return(0,f.jsxs)("div",{className:"flex flex-col p-0",children:[(0,f.jsx)(ea,{doSearch:i,state:e.state,dispatch:e.dispatch}),(0,f.jsx)(er,{state:e.state,results:t,onItemClick:a})]})},eh=e=>{if(e<=0)return"unknown";let t=[];for(let s of[{period:"day",divisor:86400},{period:"hour",divisor:3600},{period:"min",divisor:60},{period:"sec",divisor:1}]){let a=Math.floor(e/s.divisor);if(e%=s.divisor,0!==a){let e=a>1?s.period+"s":s.period;t.push("".concat(a," ").concat(e))}}return t.join(" ")},eu=e=>{if(e<=0)return"-00:00:01";let t=[];for(let s of[3600,60,1]){let a=Math.floor(e/s);e%=s,(0!==a||t.length||60==s)&&t.push(a.toString().padStart(2,"0"))}return t.join(":")},em=e=>{let t=[],s=e.result,a=e.index;if(s.finished)return(0,f.jsx)(ex,{detail:"Finished"},"done:"+a);if(s.sizeDetails&&t.push((0,f.jsx)(ex,{label:"Size",detail:s.sizeDetails},"size:"+a)),s.eta){let e=eh(s.eta),i="".concat(e," (").concat((100*s.percentDone).toFixed(2),"%)");t.push((0,f.jsx)(ex,{label:"Eta",detail:i},"eta:"+a))}return s.rateDetails&&t.push((0,f.jsx)(ex,{label:"Rate",detail:s.rateDetails},"rate:"+a)),s.processDetails&&t.push((0,f.jsx)(ex,{detail:s.processDetails},"proc:"+a)),(0,f.jsx)(f.Fragment,{children:t})},ex=e=>{let t=void 0===e.label?"":"".concat(e.label,": ");return(0,f.jsxs)("p",{className:"text-gray-600 text-xs",children:[t,e.detail,"\xa0"]})},eg=e=>{let[t,s]=(0,j.useState)([]);(0,j.useEffect)(()=>{if(e.isActive){let t=new ec(e.host);(async()=>{await t.list(s)})();let a=setInterval(async()=>{await t.list(s)},2e3);return()=>clearInterval(a)}},[e.isActive,e.host]);let a=async t=>{let s=new ec(e.host);await s.delete(t)};return(0,f.jsx)("div",{className:"p-0",children:(0,f.jsx)(ev,{results:t,onItemClick:a})})},ev=e=>{let t=e.results.map((t,s,a)=>{let i=K;return s!==a.length-1&&(i+=" border-b"),(0,f.jsxs)("li",{className:i,onClick:s=>e.onItemClick(t),children:[(0,f.jsx)("p",{children:t.displayName}),(0,f.jsx)(em,{result:t,index:s})]},"search:"+s)});return 0==e.results.length?(0,f.jsx)("p",{className:"px-1 py-2",children:"No results"}):(0,f.jsx)("ul",{className:H,children:t})},ep=e=>{let t="modal-overlay",s=s=>{var a;(null===(a=s.target)||void 0===a?void 0:a.id)===t&&e.onClose()};return(0,f.jsx)("div",{id:t,className:"z-30 fixed top-0 left-0 mb-auto h-fill-viewport w-full bg-gray-900 bg-opacity-50 dark:bg-opacity-80",onClick:s,children:(0,f.jsxs)("div",{className:"z-35 inset-x-0 max-w-max mx-auto flex flex-col fixed top-8 h-max max-h-screen shadow transition-opacity duration-100 rounded divide-y divide-gray-100 border border-gray-600 bg-white text-gray-900 dark:border-none dark:bg-gray-700 dark:text-white",children:[(0,f.jsxs)("div",{className:"flex items-start justify-between bg-gray-50 rounded-t dark:border-gray-600 border-b p-2",children:[(0,f.jsx)("h3",{className:"mt-1 ml-1 text-l font-medium text-gray-900 dark:text-white",children:e.title}),(0,f.jsx)("button",{"aria-label":"Close",className:"ml-auto inline-flex items-center rounded-lg bg-transparent p-1.5 text-sm text-gray-400 hover:bg-gray-200 hover:text-gray-900 dark:hover:bg-gray-600 dark:hover:text-white",type:"button",onClick:t=>e.onClose(),children:(0,f.jsx)("svg",{stroke:"currentColor",fill:"none",strokeWidth:"0",viewBox:"0 0 24 24","aria-hidden":"true",className:"h-5 w-5",height:"1em",width:"1em",xmlns:"http://www.w3.org/2000/svg",children:(0,f.jsx)("path",{strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"2",d:"M6 18L18 6M6 6l12 12"})})})]}),(0,f.jsx)("div",{className:"mb-auto overflow-y-scroll",children:e.children})]})})},ey=e=>(0,f.jsx)(ep,{onClose:e.onClose,title:e.title,children:(0,f.jsx)("div",{className:"p-2",children:e.children})}),ef=e=>{let[t,s]=(0,j.useState)(""),a=async s=>{if(s.preventDefault(),""===t){N("Must enter a new name");return}e.onClose(),await e.player.renameVideo(e.video,t)};return(0,f.jsx)(ep,{onClose:e.onClose,children:(0,f.jsxs)("div",{className:"p-4",children:[(0,f.jsxs)("div",{className:"mb-2",children:[(0,f.jsx)("div",{className:"mb-2 block",children:(0,f.jsx)(k.__,{htmlFor:"newName",value:"New Name"})}),(0,f.jsx)(k.oi,{id:"newName",placeholder:e.video,value:t,onChange:e=>s(e.target.value),required:!0})]}),(0,f.jsxs)("div",{className:"flex flex-wrap items-center gap-4",children:[(0,f.jsx)(k.zx,{onClick:e=>a(e),children:"Ok"}),(0,f.jsx)(k.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})},ej=e=>{let[t,s]=(0,j.useState)(""),[a,i]=(0,j.useState)([]);(0,j.useEffect)(()=>{let t=async()=>{let t=await e.player.getAvailableConversions();return t.length>0&&s(t[0].name),i(t)};t().catch(e=>P(e,"getConversions"))},[e.player]);let r=async s=>{if(""===t){N("Select a conversion method");return}e.onClose(),await e.player.convertVideo(e.video,t)},o=e=>{let t=e.currentTarget;s(t.value)},l=a.map((e,t)=>(0,f.jsxs)("div",{className:"flex items-center gap-2",children:[(0,f.jsx)(k.Y8,{id:e.name,name:"conversion",value:e.name,defaultChecked:0===t,onChange:o}),(0,f.jsx)(k.__,{htmlFor:e.name,children:e.name})]},t));return(0,f.jsx)(ep,{onClose:e.onClose,children:(0,f.jsxs)("div",{className:"p-4",children:[(0,f.jsxs)("div",{className:"mb-2",children:[(0,f.jsx)("div",{className:"mb-2 block",children:(0,f.jsx)(k.__,{htmlFor:"conversion",value:"Convert Video"})}),(0,f.jsx)("div",{className:"flex flex-col gap-4 min-w-[16em]",children:l})]}),(0,f.jsxs)("div",{className:"flex pt-4 flex-wrap items-center gap-4",children:[(0,f.jsx)(k.zx,{onClick:e=>r(e),children:"Ok"}),(0,f.jsx)(k.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})};var ek=s(9274);let ew=e=>{let t;let s=e.player,a=null==e?void 0:e.video;return t=void 0!==a?(0,f.jsx)(eb,{onClick:()=>s.playVideo(a),iconClass:ek.V1r}):(0,f.jsxs)(f.Fragment,{children:[(0,f.jsx)(eb,{onClick:()=>s.seek(-15),iconClass:ek.Dfd}),(0,f.jsx)(eb,{onClick:()=>s.togglePause(),iconClass:ek.V1r}),(0,f.jsx)(eb,{onClick:()=>s.seek(15),iconClass:ek.TuD})]}),(0,f.jsx)("div",{className:"flex gap-6 p-2 w-full items-center justify-center rounded-b-lg border-t bg-gray-50",children:t})},eb=e=>{let t=j.createElement(e.iconClass,{color:"gray",className:"h-6 w-6"}),s=()=>{e.onClick()};return(0,f.jsx)(k.zx,{color:"gray",className:"border-gray-700",outline:!0,pill:!0,onClick:()=>s(),children:t})};class eC{constructor(e){this.getDuration=()=>eh(this.metadata.duration),this.getSize=()=>"".concat(this.metadata.width,"x").concat(this.metadata.height),this.video=e.video,this.collection=e.collection,this.description=e.description,this.series=e.series,this.thumbnail=e.thumbnail,this.metadata=e.metadata}}let eN=(0,f.jsx)(f.Fragment,{});class eS{constructor(e){this.name=()=>{var e;return void 0!==this.video?this.video:void 0!==(null===(e=this.lastMessage)||void 0===e?void 0:e.State)?this.lastMessage.State.video:"???"},this.getCollection=()=>{var e;return void 0!==this.collection?this.collection:void 0!==(null===(e=this.lastMessage)||void 0===e?void 0:e.State)?this.lastMessage.State.collection:"???"},this.getDetails=async()=>{try{let e=await this.player.fetchDetails(this.name(),this.getCollection());if(void 0!==e.Video)return new eC(e.Video)}catch(e){P(e,"getDetails: ".concat(this.name(),", ").concat(this.getCollection()))}return null},this.getUrl=()=>{let e=this.getCollection(),t="/api/stream";return""!==e?"".concat(t,"/").concat(e,"/").concat(this.name()):"".concat(t,"/").concat(this.name())},this.showProgress=()=>{var e;let t=null===(e=this.lastMessage)||void 0===e?void 0:e.State;return!!t&&t.video==this.name()&&t.collection==this.getCollection()},void 0!==e.lastMessage&&(this.lastMessage=e.lastMessage),this.name()!==e.video&&(this.video=e.video,this.collection=e.collection),this.player=e.player,this.setDialog=e.setDialog,this.back=e.back}}let eT=e=>{let[t,s]=(0,j.useState)(null),a=new eS(e);return(0,j.useEffect)(()=>{let e=async()=>{try{let e=await a.getDetails();s(e)}catch(e){P(e,"fetchData")}};e()},[e.video,e.collection]),(0,f.jsxs)("div",{className:"max-w-screen-md border border-gray-200 rounded-lg shadow dark:bg-gray-800 dark:border-gray-700",children:[(0,f.jsxs)("div",{className:"p-2",children:[(0,f.jsx)("a",{href:"#",children:(0,f.jsx)("img",{className:"mx-auto object-cover h-full w-52 rounded-lg",src:"/api/thumbnails/".concat(null==t?void 0:t.thumbnail),alt:"image from ".concat(a.name())})}),(0,f.jsxs)("div",{className:"mt-2",children:[(0,f.jsx)("div",{className:"float-right",children:(0,f.jsx)(e_,{...a})}),(0,f.jsx)("a",{href:"#",children:(0,f.jsx)("h5",{className:"mb-2 font-bold tracking-tight text-gray-900 dark:text-white",children:a.name()})}),(0,f.jsx)("div",{className:"mb-3 font-medium text-sm text-gray-700 dark:text-gray-400",children:a.showProgress()?(0,f.jsx)(eD,{message:a.lastMessage}):eE(t)})]})]}),(0,f.jsx)(ew,{player:a.player,video:a.video})]})},eE=e=>{try{return(0,f.jsxs)("div",{children:[(0,f.jsxs)("p",{children:["Duration: ",(null==e?void 0:e.getDuration())||"?"]}),(0,f.jsxs)("p",{children:["Size: ",(null==e?void 0:e.getSize())||"?"]})]})}catch(t){return P(t,"getDescription: ".concat(e)),(0,f.jsx)("div",{children:JSON.stringify(t)})}},eD=e=>{var t;let s=null===(t=e.message)||void 0===t?void 0:t.State;if(void 0===s)return(0,f.jsx)(f.Fragment,{});let a=100*s.currentTime/s.duration,i=eu(s.currentTime),r=eu(s.duration),o="".concat(i," / ").concat(r);return(0,f.jsxs)("div",{className:"flex flex-col",children:[(0,f.jsx)("p",{className:"mb-2",children:o}),(0,f.jsx)(k.Ex,{labelText:!1,progress:a})]})},e_=e=>(0,f.jsxs)(k.Lt,{label:"...",placement:"left-start",arrowIcon:!1,inline:!0,children:[(0,f.jsx)(k.Lt.Item,{onClick:()=>eV(e),children:"Rename"}),(0,f.jsx)(k.Lt.Item,{onClick:()=>eR(e),children:"Convert..."}),(0,f.jsx)(k.Lt.Item,{onClick:()=>eP(e),children:"Download"}),(0,f.jsx)(k.Lt.Item,{onClick:()=>eA(e),children:"Delete"})]}),eV=e=>{let t=()=>e.setDialog(eN),s=(0,f.jsx)(ef,{onClose:t,video:e.name(),player:e.player});e.setDialog(s)},eR=e=>{let t=()=>e.setDialog(eN),s=(0,f.jsx)(ej,{onClose:t,video:e.name(),player:e.player});e.setDialog(s)},eA=e=>{e.player.deleteVideo(e.name(),e.back)},eP=e=>{if(void 0!==e.video){let t=document.createElement("a");t.download=e.video,t.href=e.getUrl(),document.body.appendChild(t),t.click(),document.body.removeChild(t)}},eO=e=>{var t,s;let a="flex rounded-full w-14 h-14 justify-center items-center border-2 border-gray-300 bg-primary-700 hover:bg-primary-900 shadow-lg",i="h-6 w-6",r=a+(void 0===(null===(t=e.lastMessage)||void 0===t?void 0:t.State)?" invisible":""),o=void 0===(null===(s=e.lastMessage)||void 0===s?void 0:s.State)?(0,f.jsx)(f.Fragment,{}):(0,f.jsx)(w.$In,{color:"white",className:i});return(0,f.jsxs)("div",{className:"z-20 p-1 fixed flex flex-col-reverse gap-4 right-2 bottom-2 group",children:[(0,f.jsx)("div",{className:a,onClick:()=>e.showTasks(),children:(0,f.jsx)(w.EKd,{color:"white",className:i})}),(0,f.jsx)("div",{className:a,onClick:()=>e.showSearch(),children:(0,f.jsx)(w.G4C,{color:"white",className:i})}),(0,f.jsx)("div",{className:r,onClick:()=>e.showCurrentVideo(),children:o})]})},eL={term:"",engine:W,results:[],lastSearch:""};(c=p||(p={}))[c.Videos=0]="Videos",c[c.CurrentVideo=1]="CurrentVideo",c[c.Search=2]="Search",c[c.Tasks=3]="Tasks",c[c.VideoDetail=4]="VideoDetail";let eI=e=>{let[t,s]=(0,j.useState)(""),[a,i]=(0,j.useState)(p.Videos),[r,o]=(0,j.useState)(!1),[l,n]=(0,j.useState)(),[c,d]=(0,j.useState)((0,f.jsx)(f.Fragment,{})),[h,u]=(0,j.useState)(null),[m,x]=(0,j.useReducer)(es,eL),g=new $(t,s,e.host,"");(0,j.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host;new I(()=>new WebSocket("ws://".concat(t,"/api/control/ws")),e=>{n(e)})},[e.host]);let v=e=>{u(e),i(p.VideoDetail)};return b.setStateFunction(o),(0,f.jsxs)("div",{className:"lg:container lg:mx-auto flex flex-col h-fill-viewport w-full",children:[(0,f.jsx)(D,{show:r}),(()=>{let s=()=>i(p.Videos);switch(a){case p.Search:return(0,f.jsx)(ey,{title:"Search",onClose:s,children:(0,f.jsx)(ed,{host:e.host,state:m,dispatch:x})});case p.Tasks:return(0,f.jsx)(ey,{title:"Running Tasks",onClose:s,children:(0,f.jsx)(eg,{host:e.host,isActive:!0})});case p.CurrentVideo:return(0,f.jsx)(ey,{title:"Current Video",onClose:s,children:(0,f.jsx)(eT,{player:g,setDialog:d,lastMessage:l,back:s})});case p.VideoDetail:if(null!==h){let e=()=>{u(null),i(p.Videos)};return(0,f.jsx)(ey,{title:"Video Details",onClose:e,children:(0,f.jsx)(eT,{video:h,collection:t,setDialog:d,back:()=>u(null),player:g,lastMessage:l})})}}return(0,f.jsx)(f.Fragment,{})})(),c,(0,f.jsx)("div",{className:"p-1 overflow-y-auto",children:(0,f.jsx)(Z,{videoPlayer:g,isActive:a===p.Videos,showVideoDetails:v})}),(0,f.jsx)(eO,{lastMessage:l,showCurrentVideo:()=>i(p.CurrentVideo),showSearch:()=>i(p.Search),showTasks:()=>i(p.Tasks)})]})};var eM=s(3454);let eU=eM.env.API_URL;eM.env.FORCE_PLAYER_MODE;let eF=new class{constructor(e){this.getHost=()=>void 0!==this.host?this.host:null,this.get=async(e,t)=>{let s=await fetch(this.makeUrl(e),t);if(s.status!==x.OK)throw Error("GET ".concat(e," returned ").concat(s.status," ").concat(s.statusText));try{return await s.json()}catch(s){if(void 0===t)return this.get(e,{cache:"reload"});throw P(s,"could not fetch ".concat(e)),s}},this.post=async(e,t)=>this.send("POST",e,JSON.stringify(t)),this.put=async(e,t)=>this.send("PUT",e,JSON.stringify(t)),this.send=async(e,t,s)=>fetch(this.makeUrl(t),{method:e,body:s,headers:{"Content-Type":"application/json"}}),this.delete=async e=>fetch(this.makeUrl(e),{method:"DELETE"}),this.makeUrl=e=>void 0!==this.host?"http://".concat(this.host,"/api/").concat(e):"/api/".concat(e),this.host=e}}(eU);_=new class{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}(eF),(d=y||(y={}))[d.Unknown=1]="Unknown",d[d.Video=2]="Video",d[d.Remote=3]="Remote";let ez=()=>{let[e,t]=(0,j.useState)(y.Unknown);return((0,j.useEffect)(()=>{let e=window.navigator.userAgent,s=new URLSearchParams(location.search);e.includes("SMART-TV")||e.includes("SmartTV")||s.has("player")?(R("detected smart-tv: ".concat(e,", ").concat(s)),t(y.Video)):(R("detected normal browser: ".concat(e)),t(y.Remote))},[]),e==y.Video)?(0,f.jsx)(et,{host:eF}):e==y.Remote?(0,f.jsx)(eI,{host:eF}):(0,f.jsx)("p",{children:"Loading..."})};var eB=ez}},function(e){e.O(0,[827,556,256,774,888,179],function(){return e(e.s=8312)}),_N_E=e.O()}]);