// Phone format data from FakerPHP 1.24.1.
//
// Copyright (c) 2011 François Zaninotto
// Portions Copyright (c) 2008 Caius Durling
// Portions Copyright (c) 2008 Adam Royle
// Portions Copyright (c) 2008 Fiona Burrows
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

pub(super) const INTERNATIONAL: &[&str] = &[
    "+1##########",
    "+7##########",
    "+20##########",
    "+27##########",
    "+30##########",
    "+31##########",
    "+32#########",
    "+33#########",
    "+34#########",
    "+36#########",
    "+39##########",
    "+40#########",
    "+41#########",
    "+43##########",
    "+44##########",
    "+45########",
    "+46##########",
    "+47########",
    "+48##########",
    "+49##########",
    "+51#########",
    "+52##########",
    "+53##########",
    "+54##########",
    "+55#########",
    "+56##########",
    "+57##########",
    "+58##########",
    "+60##########",
    "+61##########",
    "+62##########",
    "+63##########",
    "+64##########",
    "+65##########",
    "+66##########",
    "+81##########",
    "+82##########",
    "+84##########",
    "+86##########",
    "+90##########",
    "+91##########",
    "+92##########",
    "+93#########",
    "+94#########",
    "+95##########",
    "+98##########",
    "+211#########",
    "+212#########",
    "+213#########",
    "+216########",
    "+218#########",
    "+220#######",
    "+221#########",
    "+222########",
    "+223########",
    "+224#########",
    "+225##########",
    "+226########",
    "+227########",
    "+228########",
    "+229########",
    "+230########",
    "+231#########",
    "+232########",
    "+233#########",
    "+234##########",
    "+235########",
    "+236########",
    "+237#########",
    "+238#######",
    "+239#######",
    "+240#########",
    "+241########",
    "+242#########",
    "+243#########",
    "+244#########",
    "+245#########",
    "+246#######",
    "+247######",
    "+248#######",
    "+249#########",
    "+250#########",
    "+251#########",
    "+252#########",
    "+253########",
    "+254##########",
    "+255#########",
    "+256#########",
    "+257########",
    "+258#########",
    "+260#########",
    "+261#########",
    "+262#########",
    "+263##########",
    "+264#########",
    "+265#########",
    "+266########",
    "+267##########",
    "+268#########",
    "+269#######",
    "+290#####",
    "+291#######",
    "+297#######",
    "+298######",
    "+299######",
    "+350########",
    "+351#########",
    "+352##########",
    "+353##########",
    "+354#########",
    "+355#########",
    "+356########",
    "+357########",
    "+358##########",
    "+359#########",
    "+370########",
    "+371########",
    "+372##########",
    "+373########",
    "+374########",
    "+375##########",
    "+376#########",
    "+377#########",
    "+378##########",
    "+380##########",
    "+381##########",
    "+382#########",
    "+383#########",
    "+385#########",
    "+386########",
    "+387#########",
    "+389########",
    "+420#########",
    "+421#########",
    "+423#########",
    "+500#####",
    "+501#######",
    "+502########",
    "+503########",
    "+504########",
    "+505########",
    "+506########",
    "+507##########",
    "+508#########",
    "+509########",
    "+590#########",
    "+591#########",
    "+592#######",
    "+593##########",
    "+594#########",
    "+595##########",
    "+596#########",
    "+597#######",
    "+598##########",
    "+599########",
    "+670########",
    "+672######",
    "+673#######",
    "+674#######",
    "+675########",
    "+676#######",
    "+677#######",
    "+678#######",
    "+679#######",
    "+680#######",
    "+681#########",
    "+682#####",
    "+683#######",
    "+685##########",
    "+686########",
    "+687######",
    "+688#######",
    "+689#########",
    "+690#######",
    "+691#######",
    "+692#######",
    "+800########",
    "+808########",
    "+850##########",
    "+852#########",
    "+853########",
    "+855##########",
    "+856##########",
    "+870#########",
    "+878############",
    "+880##########",
    "+881#########",
    "+882##########",
    "+883#########",
    "+886##########",
    "+888###########",
    "+960##########",
    "+961########",
    "+962#########",
    "+963#########",
    "+964##########",
    "+965########",
    "+966##########",
    "+967#########",
    "+968#########",
    "+970##########",
    "+971##########",
    "+972##########",
    "+973########",
    "+974########",
    "+975########",
    "+976##########",
    "+977##########",
    "+979#########",
    "+992#########",
    "+993########",
    "+994#########",
    "+995#########",
    "+996#########",
    "+998#########",
];

pub(super) const INTERNATIONAL_DE_AT: &[&str] = &["+43##########"];

pub(super) const INTERNATIONAL_DE_CH: &[&str] = &["+41##########"];

pub(super) const INTERNATIONAL_EN_GB: &[&str] = &["+44##########"];

pub(super) const INTERNATIONAL_FR_FR: &[&str] = &["+33#########"];

pub(super) const INTERNATIONAL_TR_TR: &[&str] = &[
    "+90212#######",
    "+90216#######",
    "+90222#######",
    "+90224#######",
    "+90226#######",
    "+90228#######",
    "+90232#######",
    "+90236#######",
    "+90242#######",
    "+90246#######",
    "+90248#######",
    "+90252#######",
    "+90256#######",
    "+90258#######",
    "+90262#######",
    "+90264#######",
    "+90266#######",
    "+90272#######",
    "+90274#######",
    "+90276#######",
    "+90282#######",
    "+90284#######",
    "+90286#######",
    "+90288#######",
    "+90312#######",
    "+90318#######",
    "+90322#######",
    "+90324#######",
    "+90326#######",
    "+90328#######",
    "+90332#######",
    "+90338#######",
    "+90342#######",
    "+90344#######",
    "+90346#######",
    "+90348#######",
    "+90352#######",
    "+90354#######",
    "+90356#######",
    "+90358#######",
    "+90362#######",
    "+90364#######",
    "+90366#######",
    "+90368#######",
    "+90370#######",
    "+90372#######",
    "+90374#######",
    "+90376#######",
    "+90378#######",
    "+90380#######",
    "+90382#######",
    "+90384#######",
    "+90386#######",
    "+90388#######",
    "+90412#######",
    "+90414#######",
    "+90416#######",
    "+90422#######",
    "+90424#######",
    "+90426#######",
    "+90428#######",
    "+90432#######",
    "+90434#######",
    "+90436#######",
    "+90438#######",
    "+90442#######",
    "+90446#######",
    "+90452#######",
    "+90454#######",
    "+90456#######",
    "+90458#######",
    "+90462#######",
    "+90464#######",
    "+90466#######",
    "+90472#######",
    "+90474#######",
    "+90476#######",
    "+90478#######",
    "+90482#######",
    "+90484#######",
    "+90486#######",
    "+90488#######",
    "+90501#######",
    "+90502#######",
    "+90503#######",
    "+90504#######",
    "+90505#######",
    "+90506#######",
    "+90507#######",
    "+90530#######",
    "+90531#######",
    "+90532#######",
    "+90533#######",
    "+90534#######",
    "+90535#######",
    "+90536#######",
    "+90537#######",
    "+90538#######",
    "+90539#######",
    "+90541#######",
    "+90542#######",
    "+90543#######",
    "+90544#######",
    "+90545#######",
    "+90546#######",
    "+90549#######",
    "+90551#######",
    "+90552#######",
    "+90553#######",
    "+90554#######",
    "+90555#######",
    "+90556#######",
];

pub(super) const INTERNATIONAL_UK_UA: &[&str] = &[
    "+38050#######",
    "+38066#######",
    "+38068#######",
    "+38096#######",
    "+38067#######",
    "+38091#######",
    "+38092#######",
    "+38093#######",
    "+38094#######",
    "+38095#######",
    "+38096#######",
    "+38097#######",
    "+38098#######",
    "+38063#######",
    "+38099#######",
];

pub(super) const AREA_EN_US: &[&str] = &[
    "(0[1-35-9]|1[02-9]|2[03-589]|3[149]|4[08]|5[1-46]|6[0279]|7[0269]|8[13])",
    "(0[1-57-9]|1[02-9]|2[0135]|3[0-24679]|4[167]|5[12]|6[014]|8[056])",
    "(0[124-9]|1[02-579]|2[3-5]|3[0245]|4[0235]|58|6[39]|7[0589]|8[04])",
    "(0[1-57-9]|1[0235-8]|20|3[0149]|4[01]|5[19]|6[1-47]|7[013-5]|8[056])",
    "(0[1-35-9]|1[024-9]|2[03689]|[34][016]|5[017]|6[0-279]|78|8[0-29])",
    "(0[1-46-8]|1[2-9]|2[04-7]|3[1247]|4[037]|5[47]|6[02359]|7[02-59]|8[156])",
    "(0[1-68]|1[02-8]|2[08]|3[0-28]|4[3578]|5[046-9]|6[02-5]|7[028])",
    "(0[1346-9]|1[02-9]|2[0589]|3[0146-8]|4[0179]|5[12469]|7[0-389]|8[04-69])",
];

pub(super) const AREA_DE_DE: &[&str] = &[
    "(0[0-389]|0[4-6][1-68]|1[124]|1[0-9][0-9]|2[18]|2[0-9][1-9]|3[14]|3[0-35-9][0-9]|4[1]|4[02-8][0-9]|5[1]|5[02-9][0-9]|6[1]|6[02-9][0-9]|7[1]|7[2-7][0-9]|8[1]|8[02-7][0-9]|9[1]|9[02-9][0-9])",
    "(0|3[15]|3[02-46-9][1-9]|3[02-46-9][02-9][0-9]|4[015]|4[2-4679][1-8]|4[2-4679][02-9][0-9]|5[15]|5[02-46-9][1-9]|5[02-46-9][02-9][0-9]|6[15]|6[02-46-9][1-9]|6[02-46-9][02-9][0-9]|7[15]|7[2-467][1-7]|7[2-467][02-689][0-9]|8[15]|8[2-46-8][013-9]|8[2-46-8][02-9][0-9]|9[15]|9[02-46-9][1-9]|9[02-46-9][02-9][0-9])",
    "(0|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-9][0-9]|5[1]|5[02-6][0-9]|6[1]|6[02-8][0-9]|7[1]|7[02-79][0-9]|8[1]|8[02-9][0-9]|9[1]|9[02-7][0-9])",
    "(0[2-8][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][1-9]|3[1]|3[02-8][0-9]|4[1]|4[02-9][1-9]|5[1]|5[02-9][0-9]|6[1]|6[02-9][0-9]|7[1]|7[02-7][1-9]|8[1]|8[02-8][0-9]|9[1]|9[0-7][1-9])",
    "(0[02-9][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-8][0-9]|5[1]|5[02-9][0-9]|6[1]|6[2-9][0-9]|7[1]|7[02-8][1-9]|8[1]|8[02-9][1-9]|9)",
    "(0[2-8][1-6]|1[1]|1[2-9][0-9]|2[1]|2[0-7][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-8][0-9]|5[1]|5[02-8][0-9]|6[1]|6[02-8][0-9]|7[1]|7[02-7][0-9]|8[1]|8[02-5][1-9]|9[1]|9[03-7][0-9])",
    "(0[2-9][0-9]|1[1]|1[02-79][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[02-6][0-9]|5[1]|5[02-9][0-9]|6[1]|6[2-8][0-9]|7[1]|7[02-8][1-9]|8[1]|8[02-6][0-9]|9)",
    "(0[6]|0[07-9][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[02-9][0-9]|5[1]|5[02-7][0-9]|6[1]|6[02-8][1-9]|7[1]|7[02-467][0-9]|8[1]|8[02-7][0-9]|9[1]|9[02-7][0-9])",
];

pub(super) const TAIWAN: &[&str] = &[
    "+8869########",
    "+886-9##-###-###",
    "09########",
    "09##-###-###",
    "(02)########",
    "(02)####-####",
    "(0#)#######",
    "(0#)###-####",
    "(0##)######",
    "(0##)###-###",
];

pub(super) const CHINA_OPERATORS: &[&str] = &[
    "134", "135", "136", "137", "138", "139", "147", "150", "151", "152", "157", "158", "159",
    "1705", "178", "182", "183", "184", "187", "188", "130", "131", "132", "145", "155", "156",
    "1707", "1708", "1709", "1718", "1719", "176", "185", "186", "133", "153", "1700", "1701",
    "177", "180", "181", "189", "170", "171",
];

use super::{Faker, UniqueFaker};
use crate::database::{DatabaseError, DatabaseResult};

const DE_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const DE_DE_MOBILE: &[&str] = &[
    "+49{{mobileCode}}#######",
    "+49 {{mobileCode}} ### ####",
    "0{{mobileCode}}#######",
    "0{{mobileCode}} ### ####",
    "0 {{mobileCode}} ### ####",
];
const EL_CY_MOBILE: &[&str] = &["9#######"];
const EL_GR_MOBILE: &[&str] = &[
    "{{internationalCodePrefix}}{{mobileCode}}#######",
    "{{internationalCodePrefix}} {{mobileCode}} ### ####",
];
const EN_AU_MOBILE: &[&str] = &[
    "04## ### ###",
    "04##-###-###",
    "04##.###.###",
    "+61 4## ### ###",
    "+61-4##-###-###",
    "+61.4##.###.###",
];
const EN_GB_MOBILE: &[&str] = &["07#########", "07### ######", "07### ### ###"];
const EN_HK_MOBILE: &[&str] = &["5#######", "6#######", "9#######"];
const EN_IN_MOBILE: &[&str] = &[
    "+91 9#########",
    "+91 8#########",
    "+91 7#########",
    "09#########",
    "08#########",
    "07#########",
];
const EN_NZ_MOBILE: &[&str] = &["02########", "02#########", "02# ### ####", "02# #### ####"];
const EN_PH_MOBILE: &[&str] = &[
    "+63 (813) ###-####",
    "+63 (817) ###-####",
    "+63 (900) ###-####",
    "+63 (905) ###-####",
    "+63 (906) ###-####",
    "+63 (907) ###-####",
    "+63 (908) ###-####",
    "+63 (909) ###-####",
    "+63 (910) ###-####",
    "+63 (911) ###-####",
    "+63 (912) ###-####",
    "+63 (915) ###-####",
    "+63 (916) ###-####",
    "+63 (917) ###-####",
    "+63 (918) ###-####",
    "+63 (919) ###-####",
    "+63 (920) ###-####",
    "+63 (921) ###-####",
    "+63 (922) ###-####",
    "+63 (923) ###-####",
    "+63 (925) ###-####",
    "+63 (926) ###-####",
    "+63 (927) ###-####",
    "+63 (928) ###-####",
    "+63 (929) ###-####",
    "+63 (930) ###-####",
    "+63 (931) ###-####",
    "+63 (932) ###-####",
    "+63 (933) ###-####",
    "+63 (934) ###-####",
    "+63 (935) ###-####",
    "+63 (936) ###-####",
    "+63 (937) ###-####",
    "+63 (938) ###-####",
    "+63 (939) ###-####",
    "+63 (940) ###-####",
    "+63 (942) ###-####",
    "+63 (943) ###-####",
    "+63 (944) ###-####",
    "+63 (946) ###-####",
    "+63 (947) ###-####",
    "+63 (948) ###-####",
    "+63 (949) ###-####",
    "+63 (971) ###-####",
    "+63 (973) ###-####",
    "+63 (974) ###-####",
    "+63 (975) ###-####",
    "+63 (977) ###-####",
    "+63 (978) ###-####",
    "+63 (979) ###-####",
    "+63 (980) ###-####",
    "+63 (989) ###-####",
    "+63 (994) ###-####",
    "+63 (996) ###-####",
    "+63 (997) ###-####",
    "+63 (998) ###-####",
    "+63 (999) ###-####",
    "(0813) ###-####",
    "(0817) ###-####",
    "(0900) ###-####",
    "(0905) ###-####",
    "(0906) ###-####",
    "(0907) ###-####",
    "(0908) ###-####",
    "(0909) ###-####",
    "(0910) ###-####",
    "(0911) ###-####",
    "(0912) ###-####",
    "(0915) ###-####",
    "(0916) ###-####",
    "(0917) ###-####",
    "(0918) ###-####",
    "(0919) ###-####",
    "(0920) ###-####",
    "(0921) ###-####",
    "(0922) ###-####",
    "(0923) ###-####",
    "(0925) ###-####",
    "(0926) ###-####",
    "(0927) ###-####",
    "(0928) ###-####",
    "(0929) ###-####",
    "(0930) ###-####",
    "(0931) ###-####",
    "(0932) ###-####",
    "(0933) ###-####",
    "(0934) ###-####",
    "(0935) ###-####",
    "(0936) ###-####",
    "(0937) ###-####",
    "(0938) ###-####",
    "(0939) ###-####",
    "(0940) ###-####",
    "(0942) ###-####",
    "(0943) ###-####",
    "(0944) ###-####",
    "(0946) ###-####",
    "(0947) ###-####",
    "(0948) ###-####",
    "(0949) ###-####",
    "(0971) ###-####",
    "(0973) ###-####",
    "(0974) ###-####",
    "(0975) ###-####",
    "(0977) ###-####",
    "(0978) ###-####",
    "(0979) ###-####",
    "(0980) ###-####",
    "(0989) ###-####",
    "(0994) ###-####",
    "(0996) ###-####",
    "(0997) ###-####",
    "(0998) ###-####",
    "(0999) ###-####",
];
const EN_SG_MOBILE: &[&str] = &[
    "{{internationalCodePrefix}}9{{zeroToEight}}## ####",
    "{{internationalCodePrefix}} 9{{zeroToEight}}## ####",
    "9{{zeroToEight}}## ####",
    "{{internationalCodePrefix}}8{{oneToEight}}## ####",
    "{{internationalCodePrefix}} 8{{oneToEight}}## ####",
    "8{{oneToEight}}## ####",
];
const EN_ZA_MOBILE: &[&str] = &[
    "+27{{cellphoneCode}}#######",
    "0{{cellphoneCode}}#######",
    "0{{cellphoneCode}} ### ####",
    "0{{cellphoneCode}}-###-####",
];
const ES_ES_MOBILE: &[&str] = &[
    "+34 6## ## ####",
    "+34 6## ######",
    "+34 6########",
    "+34 6##-##-####",
    "+34 6##-######",
    "6## ## ####",
    "6## ######",
    "6########",
    "6##-##-####",
    "6##-######",
];
const FA_IR_MOBILE: &[&str] = &[
    "0910#######",
    "0911#######",
    "0912#######",
    "0913#######",
    "0914#######",
    "0915#######",
    "0916#######",
    "0917#######",
    "0918#######",
    "0919#######",
    "0901#######",
    "0901#######",
    "0902#######",
    "0903#######",
    "0930#######",
    "0933#######",
    "0935#######",
    "0936#######",
    "0937#######",
    "0938#######",
    "0939#######",
    "0920#######",
    "0921#######",
    "0937#######",
    "0990#######",
];
const FR_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const FR_FR_MOBILE: &[&str] = &[
    "+33 (0)6 {{phoneNumber06WithSeparator}}",
    "+33 6 {{phoneNumber06WithSeparator}}",
    "+33 (0)7 {{phoneNumber07WithSeparator}}",
    "+33 7 {{phoneNumber07WithSeparator}}",
    "06{{phoneNumber06}}",
    "07{{phoneNumber07}}",
    "06 {{phoneNumber06WithSeparator}}",
    "07 {{phoneNumber07WithSeparator}}",
];
const IT_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const MS_MY_MOBILE: &[&str] = &[
    "010-### ####",
    "011-{{zeroOneOnePrefix}}## ####",
    "012-### ####",
    "013-### ####",
    "014-{{zeroOneFourPrefix}}## ####",
    "016-### ####",
    "017-### ####",
    "018-### ####",
    "019-### ####",
];
const NB_NO_MOBILE: &[&str] = &[
    "+474#######",
    "+479#######",
    "9## ## ###",
    "4## ## ###",
    "9#######",
    "4#######",
];
const PT_PT_MOBILE: &[&str] = &["91#######", "92#######", "93#######", "96#######"];
const SV_SE_MOBILE: &[&str] = &[
    "+467########",
    "+46(0)7########",
    "+46 (0)7## ## ## ##",
    "+46 (0)7## ### ###",
    "07## ## ## ##",
    "07## ### ###",
    "07##-## ## ##",
    "07##-### ###",
    "07# ### ## ##",
    "07#-### ## ##",
    "07#-#######",
];
const TH_TH_MOBILE: &[&str] = &[
    "08# ### ####",
    "08 #### ####",
    "09# ### ####",
    "09 #### ####",
    "06# ### ####",
    "06 #### ####",
];
const FIXED_GR: &[&str] = &[
    "{{internationalCodePrefix}}21########",
    "{{internationalCodePrefix}} 21# ### ####",
    "{{internationalCodePrefix}}{{areaCode}}######",
    "{{internationalCodePrefix}} {{areaCode}} ######",
];
const FIXED_SG: &[&str] = &[
    "{{internationalCodePrefix}}6### ####",
    "{{internationalCodePrefix}} 6### ####",
    "6### ####",
];
const FIXED_MY: &[&str] = &[
    "03-#### ####",
    "04-### ####",
    "05-### ####",
    "06-### ####",
    "07-### ####",
    "08#-## ####",
    "09-### ####",
];
const FIXED_MY_COMPACT: &[&str] = &[
    "03########",
    "04#######",
    "05#######",
    "06#######",
    "07#######",
    "08#######",
    "09#######",
];
const MOBILE_MY_COMPACT: &[&str] = &[
    "010#######",
    "011{{zeroOneOnePrefix}}######",
    "012#######",
    "013#######",
    "014{{zeroOneFourPrefix}}######",
    "016#######",
    "017#######",
    "018#######",
    "019#######",
];
const LANDLINE_HK: &[&str] = &["2#######", "3#######"];
const EXTENSION_US: &[&str] = &[
    "{{areaCode}}-{{exchangeCode}}-#### x###",
    "({{areaCode}}) {{exchangeCode}}-#### x###",
    "1-{{areaCode}}-{{exchangeCode}}-#### x###",
    "{{areaCode}}.{{exchangeCode}}.#### x###",
    "{{areaCode}}-{{exchangeCode}}-#### x####",
    "({{areaCode}}) {{exchangeCode}}-#### x####",
    "1-{{areaCode}}-{{exchangeCode}}-#### x####",
    "{{areaCode}}.{{exchangeCode}}.#### x####",
    "{{areaCode}}-{{exchangeCode}}-#### x#####",
    "({{areaCode}}) {{exchangeCode}}-#### x#####",
    "1-{{areaCode}}-{{exchangeCode}}-#### x#####",
    "{{areaCode}}.{{exchangeCode}}.#### x#####",
];
const MOBILE_CODES_DE: &[&str] = &[
    "1511", "1512", "1514", "1515", "1516", "1517", "1520", "1521", "1522", "1523", "1525", "1526",
    "1529", "1570", "1573", "1575", "1577", "1578", "1579", "1590",
];
const MOBILE_CODES_GR: &[&str] = &[
    "685", "687", "688", "689", "690", "691", "693", "694", "695", "696", "697", "698", "699",
];
const AREA_CODES_GR: &[&str] = &[
    "2221", "2222", "2223", "2224", "2226", "2227", "2228", "2229", "2231", "2232", "2233", "2234",
    "2235", "2236", "2237", "2238", "2241", "2242", "2243", "2244", "2245", "2246", "2247", "2251",
    "2252", "2253", "2254", "2261", "2262", "2263", "2264", "2265", "2266", "2267", "2268", "2271",
    "2272", "2273", "2274", "2275", "2281", "2282", "2283", "2284", "2285", "2286", "2287", "2288",
    "2289", "2291", "2292", "2293", "2294", "2295", "2296", "2297", "2298", "2299", "231", "2321",
    "2322", "2323", "2324", "2325", "2327", "2331", "2332", "2333", "2341", "2343", "2351", "2352",
    "2353", "2371", "2372", "2373", "2374", "2375", "2376", "2377", "2381", "2382", "2384", "2385",
    "2386", "2391", "2392", "2393", "2394", "2395", "2396", "2397", "2399", "241", "2421", "2422",
    "2423", "2424", "2425", "2426", "2427", "2428", "2431", "2432", "2433", "2434", "2441", "2443",
    "2444", "2445", "2461", "2462", "2463", "2464", "2465", "2467", "2468", "2491", "2492", "2493",
    "2494", "2495", "251", "2521", "2522", "2523", "2524", "2531", "2532", "2533", "2534", "2535",
    "2541", "2542", "2544", "2591", "2592", "2593", "2594", "2551", "2552", "2553", "2554", "2555",
    "2556", "261", "2621", "2622", "2623", "2624", "2625", "2626", "2631", "2632", "2634", "2635",
    "2661", "2662", "2663", "2664", "2665", "2666", "2691", "2692", "2693", "2694", "2695", "2696",
    "2641", "2642", "2643", "2644", "2645", "2646", "2647", "2651", "2653", "2654", "2655", "2656",
    "2657", "2658", "2659", "2671", "2674", "2681", "2682", "2683", "2684", "2685", "271", "2721",
    "2722", "2723", "2724", "2725", "2731", "2732", "2733", "2734", "2735", "2736", "2741", "2742",
    "2743", "2744", "2745", "2746", "2747", "2751", "2752", "2753", "2754", "2755", "2757", "2761",
    "2763", "2765", "2791", "2792", "2795", "2797", "281", "2821", "2822", "2823", "2824", "2825",
    "2831", "2832", "2833", "2834", "2841", "2842", "2843", "2844", "2891", "2892", "2893", "2894",
    "2895", "2897",
];
const MY_011: &[&str] = &[
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "22", "23", "32",
];
const MY_014: &[&str] = &["2", "3", "4", "5", "6", "7", "8", "9"];
const AREA_CODES_BR: &[&str] = &[
    "11", "12", "13", "14", "15", "16", "17", "18", "19", "21", "22", "24", "27", "28", "31", "32",
    "33", "34", "35", "37", "38", "41", "42", "43", "44", "45", "46", "47", "48", "49", "51", "53",
    "54", "55", "61", "62", "63", "64", "65", "66", "67", "68", "69", "71", "73", "74", "75", "77",
    "79", "81", "82", "83", "84", "85", "86", "87", "88", "89", "91", "92", "93", "94", "95", "96",
    "97", "98", "99",
];

fn mobile_formats(locale: &str) -> Option<&'static [&'static str]> {
    Some(match locale {
        "de_CH" => DE_CH_MOBILE,
        "de_DE" => DE_DE_MOBILE,
        "el_CY" => EL_CY_MOBILE,
        "el_GR" => EL_GR_MOBILE,
        "en_AU" => EN_AU_MOBILE,
        "en_GB" => EN_GB_MOBILE,
        "en_HK" => EN_HK_MOBILE,
        "en_IN" => EN_IN_MOBILE,
        "en_NZ" => EN_NZ_MOBILE,
        "en_PH" => EN_PH_MOBILE,
        "en_SG" => EN_SG_MOBILE,
        "en_ZA" => EN_ZA_MOBILE,
        "es_ES" => ES_ES_MOBILE,
        "fa_IR" => FA_IR_MOBILE,
        "fr_CH" => FR_CH_MOBILE,
        "fr_FR" => FR_FR_MOBILE,
        "it_CH" => IT_CH_MOBILE,
        "ms_MY" => MS_MY_MOBILE,
        "nb_NO" => NB_NO_MOBILE,
        "pt_PT" => PT_PT_MOBILE,
        "sv_SE" => SV_SE_MOBILE,
        "th_TH" => TH_TH_MOBILE,
        _ => return None,
    })
}

pub(super) fn uses_us_phone_provider(locale: &str) -> bool {
    match locale {
        "en_US" | "en_CA" => true,
        "bg_BG" | "bn_BD" | "cs_CZ" | "da_DK" | "de_AT" | "de_CH" | "de_DE" | "el_CY" | "el_GR"
        | "en_AU" | "en_GB" | "en_HK" | "en_IN" | "en_NG" | "en_NZ" | "en_PH" | "en_SG"
        | "en_UG" | "en_ZA" | "es_AR" | "es_ES" | "es_PE" | "es_VE" | "fa_IR" | "fi_FI"
        | "fr_BE" | "fr_CH" | "fr_FR" | "he_IL" | "hr_HR" | "hu_HU" | "hy_AM" | "id_ID"
        | "is_IS" | "it_CH" | "it_IT" | "ja_JP" | "ka_GE" | "kk_KZ" | "ko_KR" | "lt_LT"
        | "lv_LV" | "me_ME" | "mn_MN" | "ms_MY" | "nb_NO" | "ne_NP" | "nl_BE" | "nl_NL"
        | "pl_PL" | "pt_BR" | "pt_PT" | "ro_MD" | "ro_RO" | "ru_RU" | "sk_SK" | "sl_SI"
        | "sv_SE" | "th_TH" | "tr_TR" | "uk_UA" | "vi_VN" | "zh_CN" | "zh_TW" => false,
        _ => true,
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn mobileNumber(&self) -> DatabaseResult<String> {
        if self.locale() == "ms_MY" {
            return self.mobileNumberWithOptions(true, true);
        }
        let formats = mobile_formats(self.locale()).ok_or(DatabaseError::InvalidQuery(
            "unsupported Faker mobile number locale",
        ))?;
        Ok(self.sample_phone_template(formats, true))
    }

    pub fn mobileNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.malaysian_phone_number(
            if formatting {
                MS_MY_MOBILE
            } else {
                MOBILE_MY_COMPACT
            },
            countryCodePrefix,
            formatting,
        )
    }

    pub fn fixedLineNumber(&self) -> DatabaseResult<String> {
        match self.locale() {
            "ms_MY" => self.fixedLineNumberWithOptions(true, true),
            "el_GR" => Ok(self.sample_phone_template(FIXED_GR, true)),
            "en_SG" => Ok(self.sample_phone_template(FIXED_SG, false)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker fixed line number locale",
            )),
        }
    }

    pub fn fixedLineNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.malaysian_phone_number(
            if formatting {
                FIXED_MY
            } else {
                FIXED_MY_COMPACT
            },
            countryCodePrefix,
            formatting,
        )
    }

    pub fn mobilePhoneNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "el_GR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker mobile phone number alias locale",
            ));
        }
        // The upstream compatibility formatter preserves leading whitespace.
        Ok(self.sample_phone_template(EL_GR_MOBILE, false))
    }

    /// The Brazilian formatter and its case-insensitive PHP alias in Korean.
    pub fn cellphoneNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        match self.locale() {
            "pt_BR" => Ok(self.brazilian_phone_number(&["9####-####"], formatted.unwrap_or(true))),
            "ko_KR" => Ok(self.numerify(Some("010-####-####"))),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker cellphone number locale",
            )),
        }
    }

    pub fn cellPhoneNumber(&self) -> DatabaseResult<String> {
        self.cellphoneNumber(None)
    }

    pub fn landlineNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        match self.locale() {
            "pt_BR" => Ok(self.brazilian_phone_number(
                &["2###-####", "3###-####", "4###-####"],
                formatted.unwrap_or(true),
            )),
            "en_HK" => Ok(self.sample_phone_template(LANDLINE_HK, false)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker landline number locale",
            )),
        }
    }

    pub fn phoneNumberWithExtension(&self) -> DatabaseResult<String> {
        if !uses_us_phone_provider(self.locale()) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker phone extension locale",
            ));
        }
        Ok(self.sample_phone_template(EXTENSION_US, false))
    }

    fn brazilian_phone_number(&self, formats: &[&str], formatted: bool) -> String {
        let area = sample(AREA_CODES_BR);
        let number = self.sample_phone_template(formats, false);
        if formatted {
            format!("({area}) {number}")
        } else {
            format!("{area}{}", number.replace('-', ""))
        }
    }

    fn malaysian_phone_number(
        &self,
        formats: &[&str],
        country_code_prefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        if self.locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Malaysian phone number locale",
            ));
        }
        let prefix = match (country_code_prefix, formatting) {
            (true, true) => "+6",
            (true, false) => "6",
            (false, _) => "",
        };
        Ok(format!(
            "{prefix}{}",
            self.sample_phone_template(formats, false)
        ))
    }

    fn sample_phone_template(&self, formats: &[&str], trim_start: bool) -> String {
        let mut format = sample(formats);
        let mut rendered = String::new();
        while let Some((literal, tail)) = format.split_once("{{") {
            let (token, remaining) = tail.split_once("}}").expect("fixed phone template");
            rendered.push_str(literal);
            self.append_phone_token(&mut rendered, token);
            format = remaining;
        }
        rendered.push_str(format);
        self.numerify(Some(if trim_start {
            rendered.trim_start()
        } else {
            &rendered
        }))
    }

    fn append_phone_token(&self, rendered: &mut String, token: &str) {
        let literal = match (self.locale(), token) {
            ("el_GR", "internationalCodePrefix") => sample(&["", "+30"]),
            ("en_SG", "internationalCodePrefix") => sample(&["+65", "65"]),
            ("el_GR", "mobileCode") => sample(MOBILE_CODES_GR),
            ("de_DE", "mobileCode") => sample(MOBILE_CODES_DE),
            ("en_SG", "zeroToEight") => {
                return rendered.push(char::from(b'0' + fastrand::u8(0..=8)));
            }
            ("en_SG", "oneToEight") => {
                return rendered.push(char::from(b'0' + fastrand::u8(1..=8)));
            }
            ("ms_MY", "zeroOneOnePrefix") => sample(MY_011),
            ("ms_MY", "zeroOneFourPrefix") => sample(MY_014),
            ("el_GR", "areaCode") => {
                let code = sample(AREA_CODES_GR);
                rendered.push_str(code);
                return rendered.push_str(&"####"[code.len()..]);
            }
            ("en_ZA", "cellphoneCode") => {
                let first = fastrand::u8(6..=8);
                let second = match first {
                    6 => fastrand::u8(0..=2),
                    7 => match fastrand::u8(1..=9) {
                        value @ (5 | 7) => value + 1,
                        value => value,
                    },
                    _ => fastrand::u8(1..=9),
                };
                rendered.push(char::from(b'0' + first));
                return rendered.push(char::from(b'0' + second));
            }
            ("fr_FR", "phoneNumber06" | "phoneNumber06WithSeparator") => {
                match fastrand::u8(0..3) {
                    0 => {
                        rendered.push_str(sample(&["0", "1", "2", "4", "5", "6", "7", "8"]));
                        rendered.push('#');
                    }
                    1 => {
                        rendered.push('3');
                        rendered.push(char::from(b'0' + fastrand::u8(0..=8)));
                    }
                    _ => {
                        rendered.push('9');
                        rendered.push_str(sample(&["5", "8", "9"]));
                    }
                }
                if token.ends_with("WithSeparator") {
                    " ## ## ##"
                } else {
                    "######"
                }
            }
            ("fr_FR", "phoneNumber07" | "phoneNumber07WithSeparator") => {
                rendered.push(char::from(b'0' + fastrand::u8(3..=8)));
                if token.ends_with("WithSeparator") {
                    "# ## ## ##"
                } else {
                    "#######"
                }
            }
            (_, "areaCode") => return rendered.push_str(&self.us_phone_area_code()),
            (_, "exchangeCode") => return rendered.push_str(&self.us_phone_exchange_code()),
            _ => unreachable!("fixed phone template token"),
        };
        rendered.push_str(literal);
    }
}

fn sample<'a>(values: &[&'a str]) -> &'a str {
    values[fastrand::usize(..values.len())]
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn mobileNumber(&self) -> DatabaseResult<String> {
        self.try_generate("mobileNumber", || self.0.mobileNumber())
    }

    pub fn mobileNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("mobileNumber", || {
            self.0
                .mobileNumberWithOptions(countryCodePrefix, formatting)
        })
    }

    pub fn fixedLineNumber(&self) -> DatabaseResult<String> {
        self.try_generate("fixedLineNumber", || self.0.fixedLineNumber())
    }

    pub fn fixedLineNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("fixedLineNumber", || {
            self.0
                .fixedLineNumberWithOptions(countryCodePrefix, formatting)
        })
    }

    pub fn mobilePhoneNumber(&self) -> DatabaseResult<String> {
        self.try_generate("mobilePhoneNumber", || self.0.mobilePhoneNumber())
    }

    pub fn cellphoneNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        let formatter = if self.0.locale() == "ko_KR" {
            "cellPhoneNumber"
        } else {
            "cellphoneNumber"
        };
        self.try_generate(formatter, || self.0.cellphoneNumber(formatted))
    }

    pub fn cellPhoneNumber(&self) -> DatabaseResult<String> {
        self.cellphoneNumber(None)
    }

    pub fn landlineNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("landlineNumber", || self.0.landlineNumber(formatted))
    }

    pub fn phoneNumberWithExtension(&self) -> DatabaseResult<String> {
        self.try_generate("phoneNumberWithExtension", || {
            self.0.phoneNumberWithExtension()
        })
    }
}
