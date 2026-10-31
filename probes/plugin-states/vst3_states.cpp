// vst3_states: dump the text of every state of the stepped parameters of
// one .vst3 module as JSON, plus a dense text scan of selected continuous
// parameters (hidden steps).
//   vst3_states --dump /path/to/x.vst3 --out states.json [--dense ids.txt]
// Loading follows tools/mapping/harvest/vst3_harvest.cpp. JSON goes to
// --out because plugins may write to stdout/stderr.

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/utility/stringconvert.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstmessage.h"
#include "pluginterfaces/vst/ivstunits.h"
#include "pluginterfaces/base/ipluginbase.h"

using namespace Steinberg;
using namespace Steinberg::Vst;

static const int32 MAX_STATES = 4096;  // more steps: record as truncated
static const int DENSE_N = 512;        // dense scan: DENSE_N + 1 points

static std::string u16(const String128 s) {
    return Steinberg::Vst::StringConvert::convert(s);
}

static std::string jesc(const std::string& s) {
    std::string o;
    for (char c : s) {
        switch (c) {
            case '"': o += "\\\""; break;
            case '\\': o += "\\\\"; break;
            case '\n': o += "\\n"; break;
            case '\r': o += "\\r"; break;
            case '\t': o += "\\t"; break;
            default:
                if (static_cast<unsigned char>(c) < 0x20) {
                    char b[8]; snprintf(b, 8, "\\u%04x", c); o += b;
                } else o += c;
        }
    }
    return o;
}

static std::string trunc48(const std::string& s) {
    return s.size() > 48 ? s.substr(0, 48) : s;
}

static std::string paramString(IEditController* c, ParamID id,
                               ParamValue v) {
    String128 out{};
    if (c->getParamStringByValue(id, v, out) != kResultOk)
        return "";
    return trunc48(u16(out));
}

// Program lists the controller offers (IUnitInfo): host-visible presets.
static void dumpPrograms(std::ostringstream& j, IEditController* ctrl) {
    j << ",\"program_lists\":[";
    FUnknownPtr<IUnitInfo> ui(ctrl);
    if (ui) {
        int32 n = ui->getProgramListCount();
        for (int32 i = 0; i < n; ++i) {
            ProgramListInfo info{};
            if (ui->getProgramListInfo(i, info) != kResultOk) continue;
            if (i) j << ",";
            j << "{\"id\":" << info.id << ",\"name\":\""
              << jesc(trunc48(u16(info.name))) << "\",\"programs\":[";
            for (int32 k = 0; k < info.programCount && k < MAX_STATES; ++k) {
                String128 name{};
                if (k) j << ",";
                std::string s = ui->getProgramName(info.id, k, name)
                    == kResultOk ? trunc48(u16(name)) : "";
                j << "\"" << jesc(s) << "\"";
            }
            j << "]}";
        }
    }
    j << "]";
}

// One JSON object per parameter: "states" holds [t, text] for every step.
static void dumpSteps(std::ostringstream& j, IEditController* ctrl,
                      const ParameterInfo& info) {
    j << ",\"source\":\"steps\"";
    if (info.stepCount > MAX_STATES) {
        j << ",\"truncated\":true,\"states\":[]";
        return;
    }
    j << ",\"states\":[";
    for (int32 st = 0; st <= info.stepCount; ++st) {
        double t = (double)st / info.stepCount;
        if (st) j << ",";
        j << "[" << t << ",\"" << jesc(paramString(ctrl, info.id, t))
          << "\"]";
    }
    j << "]";
}

// Dense scan: text at DENSE_N + 1 points, run-length coded as
// [t_first, t_last, text].
static void dumpDense(std::ostringstream& j, IEditController* ctrl,
                      const ParameterInfo& info) {
    j << ",\"source\":\"dense\",\"dense_n\":" << DENSE_N << ",\"runs\":[";
    std::string prev;
    double t0 = 0, tl = 0;
    bool first = true;
    for (int i = 0; i <= DENSE_N; ++i) {
        double t = (double)i / DENSE_N;
        std::string s = paramString(ctrl, info.id, t);
        if (i == 0) { prev = s; t0 = tl = t; continue; }
        if (s != prev) {
            if (!first) j << ",";
            j << "[" << t0 << "," << tl << ",\"" << jesc(prev) << "\"]";
            first = false;
            prev = s; t0 = t;
        }
        tl = t;
    }
    if (!first) j << ",";
    j << "[" << t0 << "," << tl << ",\"" << jesc(prev) << "\"]]";
}

static void dumpClass(std::ostringstream& j,
                      const VST3::Hosting::ClassInfo& ci,
                      VST3::Hosting::PluginFactory& factory,
                      IHostApplication* ctx,
                      const std::set<ParamID>& dense) {
    j << "{\"name\":\"" << jesc(trunc48(ci.name())) << "\","
      << "\"vendor\":\"" << jesc(trunc48(ci.vendor())) << "\","
      << "\"params\":[";
    IPtr<Vst::IComponent> comp =
        factory.createInstance<Vst::IComponent>(ci.ID());
    if (!comp) { j << "],\"error\":\"component create failed\"}"; return; }
    if (comp->initialize(ctx) != kResultOk) {
        j << "],\"error\":\"component init failed\"}"; return;
    }
    TUID ctrlCid{};
    IPtr<Vst::IEditController> ctrl;
    if (comp->getControllerClassId(ctrlCid) == kResultOk) {
        ctrl = factory.createInstance<Vst::IEditController>(
            VST3::UID::fromTUID(ctrlCid));
    }
    if (!ctrl)
        ctrl = FUnknownPtr<Vst::IEditController>(comp).getInterface();
    if (!ctrl) { j << "],\"error\":\"no edit controller\"}"; return; }
    if (ctrl->initialize(ctx) != kResultOk) {
        j << "],\"error\":\"controller init failed\"}"; return;
    }
    FUnknownPtr<Vst::IConnectionPoint> compCP(comp);
    FUnknownPtr<Vst::IConnectionPoint> ctrlCP(ctrl);
    if (compCP && ctrlCP) {
        compCP->connect(ctrlCP);
        ctrlCP->connect(compCP);
    }

    int32 n = ctrl->getParameterCount();
    bool first = true;
    for (int32 i = 0; i < n; ++i) {
        ParameterInfo info{};
        if (ctrl->getParameterInfo(i, info) != kResultOk) continue;
        bool stepped = info.stepCount > 0;
        if (!stepped && !dense.count(info.id)) continue;
        if (!first) j << ",";
        first = false;
        j << "{\"index\":" << i
          << ",\"id\":" << info.id
          << ",\"name\":\"" << jesc(trunc48(u16(info.title))) << "\""
          << ",\"steps\":" << info.stepCount
          << ",\"default\":" << info.defaultNormalizedValue
          << ",\"program_change\":"
          << ((info.flags & ParameterInfo::kIsProgramChange) ? "true"
                                                             : "false")
          << ",\"is_list\":"
          << ((info.flags & ParameterInfo::kIsList) ? "true" : "false");
        if (stepped) dumpSteps(j, ctrl, info);
        else dumpDense(j, ctrl, info);
        j << "}";
    }
    j << "]";
    dumpPrograms(j, ctrl);
    if (compCP && ctrlCP) {
        compCP->disconnect(ctrlCP);
        ctrlCP->disconnect(compCP);
    }
    ctrl->terminate();
    comp->terminate();
    j << "}";
}

int main(int argc, char** argv) {
    std::string path, out, denseFile;
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--dump") && i + 1 < argc)
            path = argv[++i];
        else if (!strcmp(argv[i], "--out") && i + 1 < argc)
            out = argv[++i];
        else if (!strcmp(argv[i], "--dense") && i + 1 < argc)
            denseFile = argv[++i];
    }
    if (path.empty()) {
        fprintf(stderr, "usage: vst3_states --dump <bundle> [--out f] "
                        "[--dense ids.txt]\n");
        return 2;
    }
    std::set<ParamID> dense;
    if (!denseFile.empty()) {
        std::ifstream f(denseFile);
        unsigned long long id;
        while (f >> id) dense.insert(static_cast<ParamID>(id));
    }
    std::string error;
    auto module = VST3::Hosting::Module::create(path, error);
    if (!module) {
        fprintf(stderr, "load: %s\n", error.c_str());
        return 1;
    }
    auto factory = module->getFactory();
    IPtr<Vst::HostApplication> ctx =
        IPtr<Vst::HostApplication>(new Vst::HostApplication(), true);
    std::ostringstream j;
    j.precision(10);
    j << "{\"module\":\"" << jesc(path) << "\",\"classes\":[";
    int n = 0;
    for (auto& ci : factory.classInfos()) {
        if (ci.category() != kVstAudioEffectClass) continue;
        if (n++) j << ",";
        dumpClass(j, ci, factory, ctx, dense);
    }
    j << "]}";
    std::string doc = j.str();
    if (!out.empty()) {
        std::ofstream f(out, std::ios::trunc);
        f << doc;
    } else {
        puts(doc.c_str());
    }
    return 0;
}
