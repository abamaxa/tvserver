(self.webpackChunk_N_E=self.webpackChunk_N_E||[]).push([[405],{8312:function(e,t,s){(window.__NEXT_P=window.__NEXT_P||[]).push(["/",function(){return s(7043)}])},7043:function(e,t,s){"use strict";s.r(t),s.d(t,{default:function(){return eU}});var a,i,r,l,o,n,c,d,h,u,m,x,g,v,p=s(5893),y=s(7294),f=s(8640),j=s(3854);(a=d||(d={}))[a.Error=0]="Error",a[a.Information=1]="Information",a[a.Warning=2]="Warning",a[a.Question=3]="Question";let w=new class{constructor(){this.message="",this.show=!1,this.type=d.Information,this.setStateFunction=e=>{this.setAlertVisible=e},this.showAlert=(e,t,s)=>{this.type=t,this.show=!0,this.onOk=s,this.message=O(e),void 0!==this.setAlertVisible&&this.setAlertVisible(!0)},this.hideAlert=()=>{this.show=!1,void 0!==this.setAlertVisible&&this.setAlertVisible(!1)},this.okClicked=()=>{let e=this.onOk;this.onOk=void 0,this.hideAlert(),void 0!==e&&e()}}},k=e=>{S(e,d.Error)},b=e=>{S(e,d.Warning)},C=e=>{S(e,d.Information)},N=(e,t)=>{S(e,d.Question,t)},S=(e,t,s)=>{if(void 0!==w)w.showAlert(e,t,s);else if(void 0!==s)throw"onOk cannot be set if no global AlertManager has been set";else alert(e)},T=e=>{let t;let s="mx-auto mb-4 h-14 w-14 ",a=null;return w.type===d.Information?t=(0,p.jsx)(j.if7,{className:s+"text-gray-400 dark:text-gray-200"}):w.type===d.Error?t=(0,p.jsx)(j.baL,{className:s+"text-red-400 dark:text-red-200"}):w.type===d.Question?(t=(0,p.jsx)(j.cLc,{className:s+"text-blue-400 dark:text-blue-200"}),a=(0,p.jsx)(f.zx,{outline:!0,onClick:()=>w.hideAlert(),children:"Cancel"})):t=(0,p.jsx)(j.HQH,{className:s+"text-yellow-400 dark:text-yellow-200"}),(0,p.jsxs)(f.u_,{className:"z-50",show:e.show,size:"md",popup:!0,children:[(0,p.jsx)(f.u_.Header,{}),(0,p.jsx)(f.u_.Body,{children:(0,p.jsxs)("div",{className:"text-center",children:[t,(0,p.jsx)("h3",{className:"mb-5 overflow-hidden text-lg font-normal text-gray-500 dark:text-gray-400",children:w.message}),(0,p.jsxs)("div",{className:"flex justify-center gap-4",children:[(0,p.jsx)(f.zx,{onClick:()=>w.okClicked(),children:"Ok"}),a]})]})})]})},E=null,_=(e,t,s,a)=>{let i=s?console.error:console.log,r=O(t,a);if(i("".concat(e," - ").concat(r)),null!==E){let t=[r];if(s){let e=Error().stack;void 0!==e&&(t=[...t,...e.split("\n")])}E.log_messages(e,t)}},D=e=>{_("info",e)},V=e=>{_("warning",e),b(e)},R=(e,t)=>{_("error",e,!0,t),k(e)},O=(e,t)=>{let s;return(s="string"==typeof e?e:e instanceof Error?e.message:JSON.stringify(e),void 0!==t)?"".concat(t,": ").concat(s):s};var A=s(5697);class P{constructor(e,t){if(this.socket=void 0,this.open=!1,this.listening=!1,this.onMessage=void 0,this.addListeners=()=>{if(void 0!==this.socket&&!this.listening){this.listening=!0;let e=this;this.socket.onopen=()=>{console.log("open websocket event"),void 0!==e.socket&&(e.open=!0,e.send({SendLastState:null}))},this.socket.onerror=t=>{console.log("WebSocket encountered error: ".concat(JSON.stringify(t),", closing socket")),setTimeout(e._reconnect,1e3)},this.socket.onmessage=t=>{e.onReceive(t).catch(e=>D("onReceive exception: ".concat(e)))}}},this.close=(e,t)=>{if(void 0!==this.socket){try{this.socket.close(e,t)}catch(e){}this.socket=void 0,this.open=!1,this.listening=!1}},this.send_string=e=>{void 0!==this.socket&&this.open&&this.socket.send(e)},this.send=e=>{if(void 0!==this.socket&&this.open){let t=JSON.stringify(e),s=new Blob([t],{type:"application/json"});this.socket.readyState>1&&this._reconnect();try{this.socket.send(s)}catch(e){this._reconnect(),this.socket.send(s)}}},this.onReceive=async e=>{void 0!==this.socket&&this.open&&void 0!==this.onMessage&&(e.data instanceof A.string?D(e.data):await this._parseMessage(e))},this._parseMessage=async e=>{try{let t=await new Response(e.data).text(),s=JSON.parse(t);this.onMessage(s)}catch(t){R("error ".concat(t,", received unexpected message: ").concat(e.data))}},this.isReady=()=>void 0!==this.socket&&this.open,this._reconnect=()=>{if(P.socketBuilder)try{this.close(),this.socket=P.socketBuilder(),this.addListeners()}catch(e){console.log("reconnecting websocket: ".concat(e))}},P._instance)return P._instance;P._instance=this,P.socketBuilder=e,this.onMessage=t,this.socket=e(),this.addListeners()}}class L{constructor(e,t,s){this.currentTime=e.currentTime,this.duration=e.duration,this.currentSrc=e.currentSrc,this.collection=t,this.video=s}}class I{constructor(e,t){this.remote_address="",this.message=e,void 0!==t&&(this.remote_address=t)}}class M{constructor(){this.collection="",this.parent_collection="",this.child_collections=[],this.videos=[],this.errors=[]}}(i=h||(h={})).YouTube="youtube",i.PirateBay="piratebay",(r=u||(u={})).Transmission="transmission",r.AsyncProcess="asyncprocess";class U{constructor(e){this.name=e}}class F{constructor(e){this.newName=e}}let z="text-sm font-medium text-gray-900 border border-gray-200 rounded-lg bg-white dark:bg-gray-700 dark:border-gray-600 dark:text-white",B="w-full px-4 py-2 border-gray-200 dark:border-gray-600 overflow-hidden",H="piratebay",K="youtube";(l=m||(m={}))[l.OK=200]="OK",l[l.ACCEPTED=202]="ACCEPTED",l[l.NO_CONTENT=204]="NO_CONTENT",l[l.PARTIAL_CONTENT=206]="PARTIAL_CONTENT",l[l.BAD_REQUEST=400]="BAD_REQUEST",l[l.UNAUTHORIZED=401]="UNAUTHORIZED",l[l.FORBIDDEN=403]="FORBIDDEN",l[l.NOT_FOUND=404]="NOT_FOUND",l[l.CONFLICT=409]="CONFLICT",l[l.UNPROCESSABLE_ENTITY=422]="UNPROCESSABLE_ENTITY",l[l.INTERNAL_SERVER_ERROR=500]="INTERNAL_SERVER_ERROR",l[l.NOT_IMPLEMENTED=501]="NOT_IMPLEMENTED";let W=e=>{let t=Y(e.isLast)+" text-gray-600",s=e.name,a=()=>{e.setVideoDetails(e.name)};return(0,p.jsx)("li",{className:t,onClick:e=>a(),children:(0,p.jsx)("div",{children:s})})},Y=e=>{let t=B;return e||(t+=" border-b"),t},q=e=>{let t=e.entry.child_collections.length-1,s=e.entry.videos.length-1,a=(()=>{let t=Q(!1),s=e.entry.parent_collection;return""===e.entry.collection?(0,p.jsx)(p.Fragment,{}):(0,p.jsxs)("li",{className:t,onClick:()=>e.setCurrentCollection(s),children:["<-"," Back"]},"0")})(),i=e.entry.child_collections.map((a,i)=>(0,p.jsx)(J,{isLast:i===t&&s<0,name:a,setCurrentCollection:e.setCurrentCollection},a)),r=e.entry.videos.map((t,a)=>(0,p.jsx)(W,{isLast:a===s,name:t,videoPlayer:e.videoPlayer,setVideoDetails:e.setVideoDetails},t));return(0,p.jsx)(p.Fragment,{children:(0,p.jsxs)("ul",{className:z,children:[a,i,r]})})},J=e=>{let t=Q(e.isLast);return(0,p.jsx)("li",{className:t,onClick:()=>e.setCurrentCollection(e.name),children:e.name})},Q=e=>{let t=B;return e||(t+=" border-b"),t},G=e=>{let[t,s]=(0,y.useState)(new M);return(0,y.useEffect)(()=>{let t=async()=>{try{let t=await e.videoPlayer.fetchDetails();t.Collection&&s(t.Collection)}catch(e){R(e,"fetchDetails")}};if(e.isActive){t();let e=setInterval(async()=>{await t()},2e5);return()=>clearInterval(e)}},[e.videoPlayer.getCurrentCollection,e.videoPlayer,e.isActive]),(0,p.jsx)("div",{className:"flex min-h-full flex-col items-center justify-center p-0",children:(0,p.jsx)("main",{className:"flex w-full flex-1 flex-col items-left justify-left",children:(0,p.jsx)(q,{entry:t,setCurrentCollection:e.videoPlayer.setCurrentCollection,setVideoDetails:e.showVideoDetails,videoPlayer:e.videoPlayer})})})};class X{constructor(e,t,s,a){this.getAvailableConversions=async()=>{let e=[];try{let t=await this.host.get("conversion");t.error&&R("conversions are unavailable: "+t.error),e=t.results||[]}catch(e){R(e,"VideoPlayer.getAvailableConversions")}return e},this.deleteVideo=async(e,t)=>{let s=[m.OK,m.ACCEPTED,m.NO_CONTENT];N('Delete video "'.concat(e,'?"'),async()=>{try{let a=this.makePath(e),i=await this.host.delete("media/".concat(a));-1===s.findIndex(e=>e===i.status)?R('cannot delete "'.concat(e,'": "').concat(i.statusText,'"')):void 0!==t&&t()}catch(t){R(t,"deleteVideo: ".concat(e))}})},this.renameVideo=async(e,t)=>{N('rename video "'.concat(e,'" to "').concat(t,'"?'),async()=>{try{let s=this.makePath(e),a=this.makePath(t),i=await this.host.put("media/".concat(s),new F(a));i.status!==m.OK&&R('cannot rename "'.concat(e,'" to "').concat(t,'": "').concat(i.statusText,'"'))}catch(s){R(s,'renameVideo: "'.concat(e,'" to "').concat(t,'"'))}})},this.convertVideo=async(e,t)=>{N("".concat(t,' with "').concat(e,'"?'),async()=>{try{let s=this.makePath(e),a=await this.host.post("media/".concat(s),new U(t));a.status!==m.OK&&R('cannot convert "'.concat(e,'": "').concat(a.statusText,'"'))}catch(s){R(s,"convertVideo: ".concat(t,' with "').concat(e,'"'))}})},this.setCurrentCollection=e=>{this.setCurrentCollectionHook(e)},this.getCurrentCollection=()=>this.currentCollection,this.playVideo=e=>{let t={remote_address:this.remote_address,collection:this.currentCollection,video:e};this.post("remote/play",t)},this.seek=e=>{let t=new I({Seek:{interval:e}});this.post("remote/control",t)},this.togglePause=()=>{let e=new I({TogglePause:"ok"});this.post("remote/control",e)},this.post=async(e,t)=>{try{let s=await this.host.post(e,t);if(s.status!==m.OK){C(s.statusText);return}let a=await s.json();a.errors.length>0&&C(a.errors[0])}catch(t){R(t,"post: ".concat(e))}},this.fetchDetails=async(e,t)=>{let s="media";return void 0!==t&&t?s+="/"+t:this.currentCollection&&(s+="/"+this.currentCollection),void 0!==e&&e&&(s+="/"+e),await this.host.get(s)},this.makePath=e=>this.currentCollection?"".concat(this.currentCollection,"/").concat(e):e,this.currentCollection=e,this.host=s,this.remote_address=a,this.setCurrentCollectionHook=t}}let Z=e=>{let[t,s]=(0,y.useState)(null),[a,i]=(0,y.useState)(),[r,l]=(0,y.useState)(""),o=(0,y.useRef)(null),n=new X(r,l,e.host,"");(0,y.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host,a=new P(()=>new WebSocket("ws://".concat(t,"/api/remote/ws")),e=>{if(void 0!==e.Play)i(e.Play);else if(void 0!==e.Seek&&null!==o){let t=o.current;t.currentTime=t.currentTime+e.Seek.interval}else if(void 0!==e.TogglePause&&null!==o){let e=o.current;c(e)}});s(a)},[e.host]);let c=e=>{e.paused?e.play().catch(e=>{V(e.message)}):e.pause()},d=e=>{D("getNextVideo called")},h=e=>{let t=e.target.error;t&&D("Video error: ".concat(t.message))},u=e=>{if(null!==t&&void 0!==a){let s=e.currentTarget;t.send({State:new L(s,a.collection,a.video)})}},m=e=>{console.log("video: ".concat(e)),n.playVideo(e)};return void 0!==a?(0,p.jsx)("div",{className:"bg-black h-screen w-screen",children:(0,p.jsx)("video",{className:"m-auto w-full h-screen object-contain outline-0",onEnded:e=>d(e),onError:e=>h(e),onTimeUpdate:e=>u(e),id:"video",autoPlay:!0,controls:!0,muted:!1,playsInline:!1,src:a.url,ref:o})}):(0,p.jsx)("div",{className:"bg-black h-screen w-screen flex",children:(0,p.jsx)("div",{className:"p-1 mx-auto overflow-y-auto w-1/2",children:(0,p.jsx)(G,{videoPlayer:n,isActive:!0,showVideoDetails:m})})})};(o=x||(x={})).TERM="TERM",o.RESULTS="RESULTS",o.ENGINE="ENGINE",o.LAST_SEARCH="LAST_SEARCH";let $=(e,t)=>{let{type:s,payload:a}=t;switch(s){case x.TERM:return{...e,term:a};case x.ENGINE:return{...e,engine:a};case x.RESULTS:return{...e,results:a};case x.LAST_SEARCH:return{...e,lastSearch:a};default:return e}},ee=e=>{let t=t=>{let s=t.currentTarget;e.dispatch({type:x.ENGINE,payload:s.value})},s=t=>{e.dispatch({type:x.TERM,payload:t.currentTarget.value})},a=t=>{e.state.term&&e.doSearch(e.state.term,e.state.engine),t.preventDefault()};return(0,p.jsxs)("form",{className:"items-center",onSubmit:e=>a(e),children:[(0,p.jsxs)("div",{className:"flex w-full",children:[(0,p.jsx)("label",{htmlFor:"simple-search",className:"sr-only",children:"SearchTab"}),(0,p.jsxs)("div",{className:"relative w-full",children:[(0,p.jsx)("div",{className:"absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none",children:(0,p.jsx)("svg",{"aria-hidden":"true",className:"w-5 h-5 text-gray-500 dark:text-gray-400",fill:"currentColor",viewBox:"0 0 20 20",xmlns:"http://www.w3.org/2000/svg",children:(0,p.jsx)("path",{fillRule:"evenodd",d:"M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z",clipRule:"evenodd"})})}),(0,p.jsx)("input",{type:"text",id:"search-term",onChange:s,value:e.state.term,className:"bg-gray-50 border border-gray-300 text-gray-900 text-sm rounded-lg focus:ring-blue-500 focus:border-blue-500 block w-full pl-10 p-2.5  dark:bg-gray-700 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white dark:focus:ring-blue-500 dark:focus:border-blue-500",placeholder:"SearchTab",required:!0})]}),(0,p.jsxs)("button",{type:"submit",className:"p-2.5 ml-2 text-sm font-medium text-white bg-blue-700 rounded-lg border border-blue-700 hover:bg-blue-800 focus:ring-4 focus:outline-none focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800",children:[(0,p.jsx)("svg",{className:"w-5 h-5",fill:"none",stroke:"currentColor",viewBox:"0 0 24 24",xmlns:"http://www.w3.org/2000/svg",children:(0,p.jsx)("path",{strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"2",d:"M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"})}),(0,p.jsx)("span",{className:"sr-only",children:"SearchTab"})]})]}),(0,p.jsxs)("div",{className:"flex gap-4 pt-4 px-1",children:[(0,p.jsxs)("div",{className:"flex items-center gap-2",children:[(0,p.jsx)(f.Y8,{id:"piratebay",name:"engine",value:H,defaultChecked:e.state.engine===H,onChange:t}),(0,p.jsx)(f.__,{htmlFor:"piratebay",children:"PirateBay"})]}),(0,p.jsxs)("div",{className:"flex items-center gap-2",children:[(0,p.jsx)(f.Y8,{id:"youtube",name:"engine",value:K,defaultChecked:e.state.engine===K,onChange:t}),(0,p.jsx)(f.__,{htmlFor:"youtube",children:"YouTube"})]})]})]})},et=z+" mt-4",es=e=>{let t=e.results.map((t,s,a)=>{let i=B;return s!=a.length-1&&(i+=" border-b"),(0,p.jsxs)("li",{className:i,onClick:s=>e.onItemClick(t),children:[(0,p.jsx)("p",{children:t.title}),(0,p.jsx)("p",{className:"text-gray-600 text-xs",children:t.description})]},"search:"+s)});if(0!=e.results.length)return(0,p.jsx)("ul",{className:et,children:t});{let t=e.state.lastSearch?"for ".concat(e.state.lastSearch):"";return(0,p.jsxs)("p",{className:"px-1 py-2",children:["No results ",t]})}};class ea{async query(e,t){try{let s=await this.host.get(this.url+e);null!==s.results?t(s.results):null!==s.error&&C(s.error)}catch(t){R(t,"query: ".concat(e))}}constructor(e,t){this.host=e,this.url=t}}class ei extends ea{constructor(e){super(e,"search/pirate?q=")}}class er extends ea{constructor(e){super(e,"search/youtube?q=")}}class el{async list(e){try{let t=await this.host.get("tasks");null!==t.results?e(t.results):null!==t.error&&V(t.error)}catch(e){R(e,"list")}}async add(e){try{await this.host.post("tasks",{name:e.title,link:e.link,engine:e.engine})}catch(e){R(e,"TaskManager.add")}}async delete(e){let t=[m.OK,m.ACCEPTED,m.NO_CONTENT];N('Terminate task "'.concat(e.name,'?"'),async()=>{try{let s=await this.host.delete("tasks/".concat(e.taskType,"/").concat(e.key));-1===t.findIndex(e=>e===s.status)&&R('cannot terminate task "'.concat(e.name,'": "').concat(s.statusText,'"'))}catch(e){R(e,"TaskManager.delete")}})}constructor(e){this.host=e}}let eo=e=>{let t=e.state.results,s=t=>{e.dispatch({type:x.LAST_SEARCH,payload:e.state.term}),e.dispatch({type:x.RESULTS,payload:t})},a=t=>{N("Download ".concat(t.title,"?"),async()=>{let s=new el(e.host);await s.add(t)})},i=(t,a)=>{let i;switch(a){case"piratebay":i=new ei(e.host);break;case"youtube":i=new er(e.host);break;default:R("unknown search engine ".concat(a));return}i.query(t,s)};return(0,p.jsxs)("div",{className:"flex flex-col p-0",children:[(0,p.jsx)(ee,{doSearch:i,state:e.state,dispatch:e.dispatch}),(0,p.jsx)(es,{state:e.state,results:t,onItemClick:a})]})},en=e=>{if(e<=0)return"unknown";let t=[];for(let s of[{period:"day",divisor:86400},{period:"hour",divisor:3600},{period:"min",divisor:60},{period:"sec",divisor:1}]){let a=Math.floor(e/s.divisor);if(e%=s.divisor,0!==a){let e=a>1?s.period+"s":s.period;t.push("".concat(a," ").concat(e))}}return t.join(" ")},ec=e=>{if(e<=0)return"-00:00:01";let t=[];for(let s of[3600,60,1]){let a=Math.floor(e/s);e%=s,(0!==a||t.length||60==s)&&t.push(a.toString().padStart(2,"0"))}return t.join(":")},ed=e=>{let t=[],s=e.result,a=e.index;if(s.finished)return(0,p.jsx)(eh,{detail:"Finished"},"done:"+a);if(s.sizeDetails&&t.push((0,p.jsx)(eh,{label:"Size",detail:s.sizeDetails},"size:"+a)),s.eta){let e=en(s.eta),i="".concat(e," (").concat((100*s.percentDone).toFixed(2),"%)");t.push((0,p.jsx)(eh,{label:"Eta",detail:i},"eta:"+a))}return s.rateDetails&&t.push((0,p.jsx)(eh,{label:"Rate",detail:s.rateDetails},"rate:"+a)),s.processDetails&&t.push((0,p.jsx)(eh,{detail:s.processDetails},"proc:"+a)),(0,p.jsx)(p.Fragment,{children:t})},eh=e=>{let t=void 0===e.label?"":"".concat(e.label,": ");return(0,p.jsxs)("p",{className:"text-gray-600 text-xs",children:[t,e.detail,"\xa0"]})},eu=e=>{let[t,s]=(0,y.useState)([]);(0,y.useEffect)(()=>{if(e.isActive){let t=new el(e.host);(async()=>{await t.list(s)})();let a=setInterval(async()=>{await t.list(s)},2e3);return()=>clearInterval(a)}},[e.isActive,e.host]);let a=async t=>{let s=new el(e.host);await s.delete(t)};return(0,p.jsx)("div",{className:"p-0",children:(0,p.jsx)(em,{results:t,onItemClick:a})})},em=e=>{let t=e.results.map((t,s,a)=>{let i=B;return s!==a.length-1&&(i+=" border-b"),(0,p.jsxs)("li",{className:i,onClick:s=>e.onItemClick(t),children:[(0,p.jsx)("p",{children:t.displayName}),(0,p.jsx)(ed,{result:t,index:s})]},"search:"+s)});return 0==e.results.length?(0,p.jsx)("p",{className:"px-1 py-2",children:"No results"}):(0,p.jsx)("ul",{className:z,children:t})},ex=e=>{let t="modal-overlay",s=s=>{var a;(null===(a=s.target)||void 0===a?void 0:a.id)===t&&e.onClose()};return(0,p.jsx)("div",{id:t,className:"z-30 fixed top-0 left-0 mb-auto h-fill-viewport w-full bg-gray-900 bg-opacity-50 dark:bg-opacity-80",onClick:s,children:(0,p.jsxs)("div",{className:"z-35 inset-x-0 max-w-max mx-auto flex flex-col fixed top-8 h-max max-h-screen shadow transition-opacity duration-100 rounded divide-y divide-gray-100 border border-gray-600 bg-white text-gray-900 dark:border-none dark:bg-gray-700 dark:text-white",children:[(0,p.jsxs)("div",{className:"flex items-start justify-between bg-gray-50 rounded-t dark:border-gray-600 border-b p-2",children:[(0,p.jsx)("h3",{className:"mt-1 ml-1 text-l font-medium text-gray-900 dark:text-white",children:e.title}),(0,p.jsx)("button",{"aria-label":"Close",className:"ml-auto inline-flex items-center rounded-lg bg-transparent p-1.5 text-sm text-gray-400 hover:bg-gray-200 hover:text-gray-900 dark:hover:bg-gray-600 dark:hover:text-white",type:"button",onClick:t=>e.onClose(),children:(0,p.jsx)("svg",{stroke:"currentColor",fill:"none",strokeWidth:"0",viewBox:"0 0 24 24","aria-hidden":"true",className:"h-5 w-5",height:"1em",width:"1em",xmlns:"http://www.w3.org/2000/svg",children:(0,p.jsx)("path",{strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"2",d:"M6 18L18 6M6 6l12 12"})})})]}),(0,p.jsx)("div",{className:"mb-auto overflow-y-scroll",children:e.children})]})})},eg=e=>(0,p.jsx)(ex,{onClose:e.onClose,title:e.title,children:(0,p.jsx)("div",{className:"p-2",children:e.children})}),ev=e=>{let[t,s]=(0,y.useState)(""),a=async s=>{if(s.preventDefault(),""===t){b("Must enter a new name");return}e.onClose(),await e.player.renameVideo(e.video,t)};return(0,p.jsx)(ex,{onClose:e.onClose,children:(0,p.jsxs)("div",{className:"p-4",children:[(0,p.jsxs)("div",{className:"mb-2",children:[(0,p.jsx)("div",{className:"mb-2 block",children:(0,p.jsx)(f.__,{htmlFor:"newName",value:"New Name"})}),(0,p.jsx)(f.oi,{id:"newName",placeholder:e.video,value:t,onChange:e=>s(e.target.value),required:!0})]}),(0,p.jsxs)("div",{className:"flex flex-wrap items-center gap-4",children:[(0,p.jsx)(f.zx,{onClick:e=>a(e),children:"Ok"}),(0,p.jsx)(f.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})},ep=e=>{let[t,s]=(0,y.useState)(""),[a,i]=(0,y.useState)([]);(0,y.useEffect)(()=>{let t=async()=>{let t=await e.player.getAvailableConversions();return t.length>0&&s(t[0].name),i(t)};t().catch(e=>R(e,"getConversions"))},[e.player]);let r=async s=>{if(""===t){b("Select a conversion method");return}e.onClose(),await e.player.convertVideo(e.video,t)},l=e=>{let t=e.currentTarget;s(t.value)},o=a.map((e,t)=>(0,p.jsxs)("div",{className:"flex items-center gap-2",children:[(0,p.jsx)(f.Y8,{id:e.name,name:"conversion",value:e.name,defaultChecked:0===t,onChange:l}),(0,p.jsx)(f.__,{htmlFor:e.name,children:e.name})]},t));return(0,p.jsx)(ex,{onClose:e.onClose,children:(0,p.jsxs)("div",{className:"p-4",children:[(0,p.jsxs)("div",{className:"mb-2",children:[(0,p.jsx)("div",{className:"mb-2 block",children:(0,p.jsx)(f.__,{htmlFor:"conversion",value:"Convert Video"})}),(0,p.jsx)("div",{className:"flex flex-col gap-4 min-w-[16em]",children:o})]}),(0,p.jsxs)("div",{className:"flex pt-4 flex-wrap items-center gap-4",children:[(0,p.jsx)(f.zx,{onClick:e=>r(e),children:"Ok"}),(0,p.jsx)(f.zx,{outline:!0,onClick:e.onClose,children:"Cancel"})]})]})})};var ey=s(9274);let ef=e=>{let t;let s=e.player,a=null==e?void 0:e.video;return t=void 0!==a?(0,p.jsx)(ej,{onClick:()=>s.playVideo(a),iconClass:ey.V1r}):(0,p.jsxs)(p.Fragment,{children:[(0,p.jsx)(ej,{onClick:()=>s.seek(-15),iconClass:ey.Dfd}),(0,p.jsx)(ej,{onClick:()=>s.togglePause(),iconClass:ey.V1r}),(0,p.jsx)(ej,{onClick:()=>s.seek(15),iconClass:ey.TuD})]}),(0,p.jsx)("div",{className:"flex gap-6 p-2 w-full items-center justify-center rounded-b-lg border-t bg-gray-50",children:t})},ej=e=>{let t=y.createElement(e.iconClass,{color:"gray",className:"h-6 w-6"}),s=()=>{e.onClick()};return(0,p.jsx)(f.zx,{color:"gray",className:"border-gray-700",outline:!0,pill:!0,onClick:()=>s(),children:t})};class ew{constructor(e){this.getDuration=()=>en(this.metadata.duration),this.getSize=()=>"".concat(this.metadata.width,"x").concat(this.metadata.height),this.video=e.video,this.collection=e.collection,this.description=e.description,this.series=e.series,this.thumbnail=e.thumbnail,this.metadata=e.metadata}}let ek=(0,p.jsx)(p.Fragment,{});class eb{constructor(e){this.name=()=>{var e;return void 0!==this.video?this.video:void 0!==(null===(e=this.lastMessage)||void 0===e?void 0:e.State)?this.lastMessage.State.video:"???"},this.getCollection=()=>{var e;return void 0!==this.collection?this.collection:void 0!==(null===(e=this.lastMessage)||void 0===e?void 0:e.State)?this.lastMessage.State.collection:"???"},this.getDetails=async()=>{try{let e=await this.player.fetchDetails(this.name(),this.getCollection());if(void 0!==e.Video)return new ew(e.Video)}catch(e){R(e,"getDetails: ".concat(this.name(),", ").concat(this.getCollection()))}return null},this.getUrl=()=>{let e=this.getCollection(),t="/api/stream";return""!==e?"".concat(t,"/").concat(e,"/").concat(this.name()):"".concat(t,"/").concat(this.name())},this.showProgress=()=>{var e;let t=null===(e=this.lastMessage)||void 0===e?void 0:e.State;return!!t&&t.video==this.name()&&t.collection==this.getCollection()},void 0!==e.lastMessage&&(this.lastMessage=e.lastMessage),this.name()!==e.video&&(this.video=e.video,this.collection=e.collection),this.player=e.player,this.setDialog=e.setDialog,this.back=e.back}}let eC=e=>{let[t,s]=(0,y.useState)(null),a=new eb(e);return(0,y.useEffect)(()=>{let e=async()=>{try{let e=await a.getDetails();s(e)}catch(e){R(e,"fetchData")}};e()},[e.video,e.collection]),(0,p.jsxs)("div",{className:"max-w-screen-md border border-gray-200 rounded-lg shadow dark:bg-gray-800 dark:border-gray-700",children:[(0,p.jsxs)("div",{className:"p-2",children:[(0,p.jsx)("a",{href:"#",children:(0,p.jsx)("img",{className:"mx-auto object-cover h-full w-52 rounded-lg",src:"/api/thumbnails/".concat(null==t?void 0:t.thumbnail),alt:"image from ".concat(a.name())})}),(0,p.jsxs)("div",{className:"mt-2",children:[(0,p.jsx)("div",{className:"float-right",children:(0,p.jsx)(eT,{...a})}),(0,p.jsx)("a",{href:"#",children:(0,p.jsx)("h5",{className:"mb-2 font-bold tracking-tight text-gray-900 dark:text-white",children:a.name()})}),(0,p.jsx)("div",{className:"mb-3 font-medium text-sm text-gray-700 dark:text-gray-400",children:a.showProgress()?(0,p.jsx)(eS,{message:a.lastMessage}):eN(t)})]})]}),(0,p.jsx)(ef,{player:a.player,video:a.video})]})},eN=e=>{try{return(0,p.jsxs)("div",{children:[(0,p.jsxs)("p",{children:["Duration: ",(null==e?void 0:e.getDuration())||"?"]}),(0,p.jsxs)("p",{children:["Size: ",(null==e?void 0:e.getSize())||"?"]})]})}catch(t){return R(t,"getDescription: ".concat(e)),(0,p.jsx)("div",{children:JSON.stringify(t)})}},eS=e=>{var t;let s=null===(t=e.message)||void 0===t?void 0:t.State;if(void 0===s)return(0,p.jsx)(p.Fragment,{});let a=100*s.currentTime/s.duration,i=ec(s.currentTime),r=ec(s.duration),l="".concat(i," / ").concat(r);return(0,p.jsxs)("div",{className:"flex flex-col",children:[(0,p.jsx)("p",{className:"mb-2",children:l}),(0,p.jsx)(f.Ex,{labelText:!1,progress:a})]})},eT=e=>(0,p.jsxs)(f.Lt,{label:"...",placement:"left-start",arrowIcon:!1,inline:!0,children:[(0,p.jsx)(f.Lt.Item,{onClick:()=>eE(e),children:"Rename"}),(0,p.jsx)(f.Lt.Item,{onClick:()=>e_(e),children:"Convert..."}),(0,p.jsx)(f.Lt.Item,{onClick:()=>eV(e),children:"Download"}),(0,p.jsx)(f.Lt.Item,{onClick:()=>eD(e),children:"Delete"})]}),eE=e=>{let t=()=>e.setDialog(ek),s=(0,p.jsx)(ev,{onClose:t,video:e.name(),player:e.player});e.setDialog(s)},e_=e=>{let t=()=>e.setDialog(ek),s=(0,p.jsx)(ep,{onClose:t,video:e.name(),player:e.player});e.setDialog(s)},eD=e=>{e.player.deleteVideo(e.name(),e.back)},eV=e=>{if(void 0!==e.video){let t=document.createElement("a");t.download=e.video,t.href=e.getUrl(),document.body.appendChild(t),t.click(),document.body.removeChild(t)}},eR=e=>{var t,s;let a="flex rounded-full w-14 h-14 justify-center items-center border-2 border-gray-300 bg-primary-700 hover:bg-primary-900 shadow-lg",i="h-6 w-6",r=a+(void 0===(null===(t=e.lastMessage)||void 0===t?void 0:t.State)?" invisible":""),l=void 0===(null===(s=e.lastMessage)||void 0===s?void 0:s.State)?(0,p.jsx)(p.Fragment,{}):(0,p.jsx)(j.$In,{color:"white",className:i});return(0,p.jsxs)("div",{className:"z-20 p-1 fixed flex flex-col-reverse gap-4 right-2 bottom-2 group",children:[(0,p.jsx)("div",{className:a,onClick:()=>e.showTasks(),children:(0,p.jsx)(j.EKd,{color:"white",className:i})}),(0,p.jsx)("div",{className:a,onClick:()=>e.showSearch(),children:(0,p.jsx)(j.G4C,{color:"white",className:i})}),(0,p.jsx)("div",{className:r,onClick:()=>e.showCurrentVideo(),children:l})]})},eO={term:"",engine:H,results:[],lastSearch:""};(n=g||(g={}))[n.Videos=0]="Videos",n[n.CurrentVideo=1]="CurrentVideo",n[n.Search=2]="Search",n[n.Tasks=3]="Tasks",n[n.VideoDetail=4]="VideoDetail";let eA=e=>{let[t,s]=(0,y.useState)(""),[a,i]=(0,y.useState)(g.Videos),[r,l]=(0,y.useState)(!1),[o,n]=(0,y.useState)(),[c,d]=(0,y.useState)((0,p.jsx)(p.Fragment,{})),[h,u]=(0,y.useState)(null),[m,x]=(0,y.useReducer)($,eO),v=new X(t,s,e.host,"");(0,y.useEffect)(()=>{let t=e.host.getHost()?e.host.getHost():location.host;new P(()=>new WebSocket("ws://".concat(t,"/api/control/ws")),e=>{n(e)})},[e.host]);let f=e=>{u(e),i(g.VideoDetail)};return w.setStateFunction(l),(0,p.jsxs)("div",{className:"lg:container lg:mx-auto flex flex-col h-fill-viewport w-full",children:[(0,p.jsx)(T,{show:r}),(()=>{let s=()=>i(g.Videos);switch(a){case g.Search:return(0,p.jsx)(eg,{title:"Search",onClose:s,children:(0,p.jsx)(eo,{host:e.host,state:m,dispatch:x})});case g.Tasks:return(0,p.jsx)(eg,{title:"Running Tasks",onClose:s,children:(0,p.jsx)(eu,{host:e.host,isActive:!0})});case g.CurrentVideo:return(0,p.jsx)(eg,{title:"Current Video",onClose:s,children:(0,p.jsx)(eC,{player:v,setDialog:d,lastMessage:o,back:s})});case g.VideoDetail:if(null!==h){let e=()=>{u(null),i(g.Videos)};return(0,p.jsx)(eg,{title:"Video Details",onClose:e,children:(0,p.jsx)(eC,{video:h,collection:t,setDialog:d,back:()=>u(null),player:v,lastMessage:o})})}}return(0,p.jsx)(p.Fragment,{})})(),c,(0,p.jsx)("div",{className:"p-1 overflow-y-auto",children:(0,p.jsx)(G,{videoPlayer:v,isActive:a===g.Videos,showVideoDetails:f})}),(0,p.jsx)(eR,{lastMessage:o,showCurrentVideo:()=>i(g.CurrentVideo),showSearch:()=>i(g.Search),showTasks:()=>i(g.Tasks)})]})};var eP=s(3454);let eL=eP.env.API_URL;eP.env.FORCE_PLAYER_MODE;let eI=new class{constructor(e){this.getHost=()=>void 0!==this.host?this.host:null,this.get=async(e,t)=>{let s=await fetch(this.makeUrl(e),t);if(s.status!==m.OK)throw Error("GET ".concat(e," returned ").concat(s.status," ").concat(s.statusText));try{return await s.json()}catch(s){if(void 0===t)return this.get(e,{cache:"reload"});throw R(s,"could not fetch ".concat(e)),s}},this.post=async(e,t)=>this.send("POST",e,JSON.stringify(t)),this.put=async(e,t)=>this.send("PUT",e,JSON.stringify(t)),this.send=async(e,t,s)=>fetch(this.makeUrl(t),{method:e,body:s,headers:{"Content-Type":"application/json"}}),this.delete=async e=>fetch(this.makeUrl(e),{method:"DELETE"}),this.makeUrl=e=>void 0!==this.host?"http://".concat(this.host,"/api/").concat(e):"/api/".concat(e),this.host=e}}(eL);E=new class{constructor(e){this.log=(e,t)=>{this.log_messages(e,[t])},this.log_messages=(e,t)=>{this.host.post("log",{level:e,messages:t}).catch(e=>{console.error(e)})},this.host=e}}(eI),(c=v||(v={}))[c.Unknown=1]="Unknown",c[c.Video=2]="Video",c[c.Remote=3]="Remote";let eM=()=>{let[e,t]=(0,y.useState)(v.Unknown);return((0,y.useEffect)(()=>{let e=window.navigator.userAgent,s=new URLSearchParams(location.search);e.includes("SMART-TV")||e.includes("SmartTV")||s.has("player")?(D("detected smart-tv: ".concat(e,", ").concat(s)),t(v.Video)):(D("detected normal browser: ".concat(e)),t(v.Remote))},[]),e==v.Video)?(0,p.jsx)(Z,{host:eI}):e==v.Remote?(0,p.jsx)(eA,{host:eI}):(0,p.jsx)("p",{children:"Loading..."})};var eU=eM}},function(e){e.O(0,[827,556,256,774,888,179],function(){return e(e.s=8312)}),_N_E=e.O()}]);